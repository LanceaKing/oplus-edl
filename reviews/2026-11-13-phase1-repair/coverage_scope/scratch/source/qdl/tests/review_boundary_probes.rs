mod support;

use qdl::{firehose_read, firehose_read_storage, firehose_read_welcome, parsers::firehose_parser_ack_nak, types::{FirehoseStatus, ReadRejected}};
use std::io::ErrorKind;
use support::*;

#[test]
fn welcome_single_byte_fragments_preserve_whitespace_boundary() {
    let wire = [b" \t\r\n".as_slice(), LOG, b" \t\r\n", b"<?xml version=\"1.0\"?><data><log value=\"next\"/></data>", b"\r\n"].concat();
    let input = wire.iter().map(|b| bytes(&[*b])).chain([Input::Error(ErrorKind::TimedOut)]);
    let (mut dev, trace) = device(input);
    assert_eq!(firehose_read_welcome(&mut dev).unwrap(), ["hello", "next"]);
    drop(dev);
    assert!(trace.lock().unwrap().writes.is_empty());
}

#[test]
fn read_rejection_matrix_is_conservative_under_fragmentation() {
    for rawmode in [None, Some("false"), Some("true"), Some("0"), Some("False"), Some("")] {
        for hint in [None, Some("AttemptRestart"), Some("AttemptRetry")] {
            let raw_attr = rawmode.map(|x| format!(" rawmode=\"{x}\"")).unwrap_or_default();
            let hint_attr = hint.map(|x| format!(" {x}=\"0\"")).unwrap_or_default();
            let nak = format!("<data><response value=\"NAK\"{raw_attr}{hint_attr}/></data>");
            let wire = [LOG, nak.as_bytes(), ACK].concat();
            let (mut dev, trace) = device(wire.iter().map(|b| bytes(&[*b])));
            let mut out = Vec::new();
            let err = firehose_read_storage(&mut dev, &mut out, 1, 0, 0, 0).unwrap_err();
            let safe = hint.is_none() && (rawmode.is_none() || rawmode == Some("false"));
            assert_eq!(err.downcast_ref::<ReadRejected>().is_some(), safe, "{nak}");
            assert!(out.is_empty());
            assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Ack);
            drop(dev);
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn fragmented_read_raw_xml_whitespace_is_not_discarded() {
    let raw = b" \t\r\n";
    let wire = [ACK, raw, ACK].concat();
    let (mut dev, trace) = device(wire.iter().map(|b| bytes(&[*b])));
    let mut out = Vec::new();
    firehose_read_storage(&mut dev, &mut out, 1, 0, 0, 0).unwrap();
    assert_eq!(out, raw);
    drop(dev);
    assert_no_reset(&trace);
}
