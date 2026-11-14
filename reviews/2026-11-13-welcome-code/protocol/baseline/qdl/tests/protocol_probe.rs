// Private offline review probes. Production source is unchanged.
mod support;

use std::io::{ErrorKind, Read};

use qdl::{
    firehose_program_storage, firehose_read, firehose_read_storage, firehose_read_welcome,
    parsers::firehose_parser_ack_nak,
    types::{FirehoseStatus, MutationConfirmation, QdlBackend, ReadRejected},
};
use support::*;

const END: &[u8] = b"<data><log value=\"INFO: End of supported functions\"/></data>";
const DECL: &[u8] = b"<?xml version=\"1.0\"?>";
const WS: &[u8] = b" \t\r\n";
const INITIAL: &[u8] = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
const FINAL: &[u8] = b"<data><response value=\"ACK\" rawmode=\"false\"/></data>";

fn fragmentation_cases(stream: &[u8]) -> Vec<Vec<Input>> {
    let mut cases = vec![vec![bytes(stream)]];
    cases.extend((1..stream.len()).map(|n| vec![bytes(&stream[..n]), bytes(&stream[n..])]));
    cases.push(stream.iter().map(|b| bytes(&[*b])).collect());
    cases.push(
        stream.iter().flat_map(|b| [Input::Error(ErrorKind::Interrupted), bytes(&[*b])]).collect(),
    );
    cases
}

#[test]
fn protocol_welcome_returns_before_touching_any_tail() {
    for tail in [WS, &b"\0\x0b\x0c\xff</data>"[..], ACK] {
        for input in fragmentation_cases(&[END, tail].concat()) {
            let (mut dev, trace) = device(input);
            assert_eq!(firehose_read_welcome(&mut dev).unwrap().len(), 1);
            let mut retained = vec![0; tail.len()];
            dev.read_exact(&mut retained).unwrap();
            assert_eq!(retained, tail);
            assert!(trace.lock().unwrap().writes.is_empty());
        }
    }
}

#[test]
fn protocol_declared_handoff_and_raw_are_preserved_at_all_splits() {
    let mut raw = [WS, ACK, NAK, b"</data>\0\xff"].concat();
    raw.extend(0u8..=255);
    raw.extend_from_slice(WS);
    while raw.len() % 4 != 0 {
        raw.push(b' ');
    }
    let stream = [END, WS, DECL, LOG, WS, DECL, INITIAL, &raw, WS, DECL, LOG, WS, DECL, FINAL, NAK].concat();
    for backend in [QdlBackend::Serial, QdlBackend::Usb] {
        for (case, input) in fragmentation_cases(&stream).into_iter().enumerate() {
            let (mut dev, trace) = device(input);
            dev.fh_cfg.backend = backend;
            assert_eq!(firehose_read_welcome(&mut dev).unwrap().len(), 1);
            let mut out = Vec::new();
            firehose_read_storage(&mut dev, &mut out, raw.len() / 4, 0, 0, 0).unwrap();
            assert_eq!(out, raw, "case={case}, backend={backend:?}");
            assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Nak);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn protocol_each_possible_first_raw_byte_is_preserved() {
    for first in 0u8..=255 {
        let raw = [first, b' ', b'\t', b'\r', b'\n', 0, b'<', 255];
        let stream = [END, WS, DECL, INITIAL, &raw, WS, DECL, FINAL].concat();
        let (mut dev, _) = device([bytes(&stream)]);
        firehose_read_welcome(&mut dev).unwrap();
        let mut out = Vec::new();
        firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap();
        assert_eq!(out, raw, "first={first}");
    }
}

fn failures() -> Vec<(Vec<u8>, Option<ErrorKind>, bool)> {
    vec![
        (WS.to_vec(), Some(ErrorKind::TimedOut), false),
        (WS.to_vec(), None, false),
        ([WS, DECL, b"<data><response value=\"ACK\""].concat(), Some(ErrorKind::TimedOut), false),
        ([WS, DECL, b"<data><response value=\"ACK\""].concat(), None, false),
        ([WS, DECL, LOG, WS].concat(), Some(ErrorKind::TimedOut), false),
        ([WS, DECL, NAK].concat(), None, true),
        ([WS, DECL, b"<data><response value=\"INVALID\"/></data>"].concat(), None, false),
        ([WS, DECL, b"<data><response value=\"ACK\"></data>"].concat(), None, false),
    ]
}

#[test]
fn protocol_read_initial_and_final_ack_remain_required_after_handoff() {
    let raw = b" \t\r\nDATA";
    for initial in [true, false] {
        for (case, (tail, error, nak)) in failures().into_iter().enumerate() {
            let prefix = if initial {
                END.to_vec()
            } else {
                [END, WS, DECL, INITIAL, raw].concat()
            };
            let (mut dev, trace) = device([
                bytes(&[prefix.as_slice(), tail.as_slice()].concat()),
                error.map(Input::Error).unwrap_or(Input::Eof),
            ]);
            firehose_read_welcome(&mut dev).unwrap();
            let mut out = Vec::new();
            let result = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap_err();
            assert_eq!(out, if initial { &b""[..] } else { &raw[..] }, "case={case}");
            assert_eq!(result.downcast_ref::<ReadRejected>().is_some(), initial && nak);
            if let Some(expected) = error {
                assert_eq!(result.downcast_ref::<std::io::Error>().unwrap().kind(), expected);
            }
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn protocol_program_initial_and_final_ack_remain_required_after_handoff() {
    let raw = b" \t\r\nDATA";
    for initial in [true, false] {
        for (case, (tail, error, nak)) in failures().into_iter().enumerate() {
            let prefix = if initial { END.to_vec() } else { [END, WS, DECL, INITIAL].concat() };
            let (mut dev, trace) = device([
                bytes(&[prefix.as_slice(), tail.as_slice()].concat()),
                error.map(Input::Error).unwrap_or(Input::Eof),
            ]);
            firehose_read_welcome(&mut dev).unwrap();
            let result = firehose_program_storage(&mut dev, &mut &raw[..], "probe", 2, 0, 0, "0").unwrap_err();
            assert_eq!(result.confirmation, if nak { MutationConfirmation::Rejected } else { MutationConfirmation::Unknown }, "case={case}");
            if let Some(expected) = error {
                assert_eq!(result.cause.downcast_ref::<std::io::Error>().unwrap().kind(), expected);
            }
            let record = trace.lock().unwrap();
            assert_eq!(record.writes.len(), if initial { 1 } else { 2 });
            if !initial { assert!(record.accepted.ends_with(raw)); }
            drop(record);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn protocol_prefixed_initial_nak_preserves_the_next_command_boundary() {
    for attrs in ["", " rawmode=\"false\""] {
        let nak = format!("<data><response value=\"NAK\"{attrs}/></data>");
        let stream = [END, WS, DECL, nak.as_bytes(), WS, DECL, ACK].concat();
        for input in fragmentation_cases(&stream) {
            let (mut dev, trace) = device(input);
            firehose_read_welcome(&mut dev).unwrap();
            let mut out = Vec::new();
            let error = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap_err();
            assert!(error.downcast_ref::<ReadRejected>().is_some());
            assert!(out.is_empty());
            assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Ack);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
        }
    }
}

#[test]
fn protocol_whitespace_inside_partial_xml_stays_in_document() {
    for partial in [
        &b"\r\n<?xml version=\"1.0\"?><data><response \r\n"[..],
        &b"\r\n<?xml version=\"1.0\"?><data><response value=\"ACK\"/>\r\n"[..],
        &b"\r\n<?xml version=\"1.0\"?><data>\r\n"[..],
    ] {
        for input in fragmentation_cases(&[END, partial].concat()) {
            let mut input = input;
            input.push(Input::Error(ErrorKind::TimedOut));
            let (mut dev, _) = device(input);
            firehose_read_welcome(&mut dev).unwrap();
            let error = firehose_read(&mut dev, firehose_parser_ack_nak).unwrap_err();
            assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::TimedOut);
        }
    }
}
