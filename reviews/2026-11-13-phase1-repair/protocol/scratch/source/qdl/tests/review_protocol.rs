// Independent review probes; not part of the submitted source snapshot.
mod support;

use qdl::{
    firehose_patch, firehose_read, firehose_read_storage, firehose_read_welcome,
    firehose_reset, firehose_set_bootable, parsers::firehose_parser_ack_nak,
    types::{FirehoseResetMode, FirehoseStatus, MutationConfirmation, ReadRejected},
};
use std::io::{BufRead, ErrorKind};
use support::*;

#[test]
fn initial_nak_classification_is_stable_at_every_split_and_keeps_tail() {
    for attributes in [
        "", " rawmode=\"false\"", " AttemptRestart=\"0\"",
        " AttemptRestart=\"\"", " AttemptRetry=\"0\"",
        " AttemptRetry=\"invalid\" rawmode=\"false\"", " rawmode=\"true\"",
        " rawmode=\"False\"", " rawmode=\"0\"", " rawmode=\" false \"",
        " AttemptRestart=\"1\" AttemptRetry=\"1\" rawmode=\"false\"",
    ] {
        let xml = format!("<data><response value=\"NAK\"{attributes}/></data>");
        for split in 1..xml.len() {
            let (mut dev, trace) = device([
                bytes(LOG), bytes(&xml.as_bytes()[..split]),
                bytes(&[&xml.as_bytes()[split..], b"TAIL"].concat()),
            ]);
            let mut out = Vec::new();
            let error = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap_err();
            let ordinary = attributes.is_empty() || attributes == " rawmode=\"false\"";
            assert_eq!(error.downcast_ref::<ReadRejected>().is_some(), ordinary, "{attributes} split={split}: {error}");
            assert!(out.is_empty());
            assert_eq!(dev.fill_buf().unwrap(), b"TAIL");
            drop(dev);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn final_read_nak_never_becomes_continueable_rejection() {
    for attributes in ["", " rawmode=\"false\"", " AttemptRestart=\"1\"", " AttemptRetry=\"1\"", " rawmode=\"true\""] {
        let xml = format!("<data><response value=\"NAK\"{attributes}/></data>");
        let (mut dev, trace) = device([bytes(ACK), bytes(b"12345678"), bytes(xml.as_bytes())]);
        let mut out = Vec::new();
        let error = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap_err();
        assert!(error.downcast_ref::<ReadRejected>().is_none());
        assert_eq!(out, b"12345678");
        drop(dev);
        assert_eq!(trace.lock().unwrap().writes.len(), 1);
        assert_no_reset(&trace);
    }
}

#[test]
fn other_modifying_commands_keep_complete_hinted_nak_rejected() {
    for attributes in ["AttemptRestart=\"1\"", "AttemptRetry=\"1\"", "rawmode=\"true\"", "rawmode=\"invalid\""] {
        for operation in 0..3 {
            let xml = format!("<data><response value=\"NAK\" {attributes}/></data>");
            let (mut dev, trace) = device([bytes(xml.as_bytes())]);
            let error = match operation {
                0 => firehose_patch(&mut dev, 0, 0, 0, 4, "0", "0"),
                1 => firehose_set_bootable(&mut dev, 0),
                _ => firehose_reset(&mut dev, &FirehoseResetMode::Reset, 0),
            }.unwrap_err();
            assert_eq!(error.confirmation, MutationConfirmation::Rejected);
            drop(dev);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
        }
    }
}

#[test]
fn welcome_whitespace_and_xml_are_stable_at_every_split() {
    let stream = b" \t\r\n<?xml version=\"1.0\"?><data><log value=\"one\"/></data>\r\n \t<?xml version=\"1.0\"?><data><log value=\"two\"/></data>\n\t ";
    for split in 1..stream.len() {
        let (mut dev, trace) = device([bytes(&stream[..split]), Input::Error(ErrorKind::Interrupted), bytes(&stream[split..]), Input::Error(ErrorKind::TimedOut)]);
        assert_eq!(firehose_read_welcome(&mut dev).unwrap(), ["one", "two"], "split={split}");
        assert!(trace.lock().unwrap().writes.is_empty());
    }
    let mut input: Vec<_> = stream.iter().map(|byte| bytes(&[*byte])).collect();
    input.push(Input::Error(ErrorKind::TimedOut));
    let (mut dev, _) = device(input);
    assert_eq!(firehose_read_welcome(&mut dev).unwrap(), ["one", "two"]);
}

#[test]
fn welcome_whitespace_never_masks_complete_invalid_or_partial_xml() {
    for invalid in [
        &b"<data><log value=\"bad\"></data>"[..],
        b"<data><log value=\"bad\"/>\x0b</data>",
        b"\x0c<data><log value=\"bad\"/></data>",
        b"<data><log value=\"bad\"/><response value=\"ACK\"/></data>",
        b"<?xml version=\"1.0\"?><data><log",
        b"<data><log value=\"bad\"/>",
        b"<data><log value=\"bad\"/></data",
        b"\xc2\xa0",
    ] {
        let (mut dev, _) = device([bytes(&[LOG, b" \t\r\n", invalid].concat()), Input::Error(ErrorKind::TimedOut)]);
        assert!(firehose_read_welcome(&mut dev).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn welcome_accepts_boundary_space_without_relaxing_document_limit_or_eof() {
    let spaces = vec![b' '; 8192];
    let (mut dev, _) = device([bytes(&[&spaces, LOG, &spaces].concat()), Input::Error(ErrorKind::TimedOut)]);
    dev.fh_cfg.xml_buf_size = LOG.len();
    assert_eq!(firehose_read_welcome(&mut dev).unwrap(), ["hello"]);
    let (mut dev, _) = device([bytes(&[&spaces, LOG].concat())]);
    dev.fh_cfg.xml_buf_size = LOG.len() - 1;
    assert!(firehose_read_welcome(&mut dev).unwrap_err().to_string().contains("receive limit"));
    let (mut dev, _) = device([bytes(&[LOG, b" \t\r\n"].concat()), Input::Eof]);
    assert_eq!(firehose_read_welcome(&mut dev).unwrap_err().downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
}

#[test]
fn all_whitespace_raw_storage_survives_every_split() {
    let stream = [ACK, b" \t\r\n \t\r\n", ACK].concat();
    for split in 1..stream.len() {
        let (mut dev, trace) = device([bytes(&stream[..split]), bytes(&stream[split..])]);
        let mut out = Vec::new();
        firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap();
        assert_eq!(out, b" \t\r\n \t\r\n", "split={split}");
        assert_eq!(trace.lock().unwrap().writes.len(), 1);
    }
}

#[test]
fn sentinel_welcome_trailing_whitespace_handoff_diagnostic() {
    let end = b"<data><log value=\"INFO: End of supported functions\"/></data>\r\n";
    let declared_ack = b"<?xml version=\"1.0\"?><data><response value=\"ACK\"/></data>";
    let (mut dev, _) = device([bytes(end), bytes(declared_ack)]);
    assert_eq!(firehose_read_welcome(&mut dev).unwrap().len(), 1);
    assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Ack);
}
