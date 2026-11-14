// Reviewer-only offline probes, outside the immutable snapshot.
mod support;
use qdl::{firehose_read, firehose_read_storage, parsers::firehose_parser_ack_nak, types::FirehoseStatus};
use support::*;
const INITIAL: &[u8] = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
const FINAL: &[u8] = b"<data><response value=\"ACK\" rawmode=\"false\"/></data>";
const DECL: &[u8] = b"<?xml version=\"1.0\"?>";

fn run_case(name: &str, raw: &[u8], separator: &[u8], declaration: &[u8]) {
    let trim = std::env::var("REVIEW_EXPECT_TRIM").unwrap() == "1";
    let expect_success = trim || declaration.is_empty();
    let stream = [INITIAL, raw, separator, declaration, FINAL, NAK].concat();
    let mut fragments = vec![vec![bytes(&stream)]];
    fragments.extend((1..stream.len()).map(|split| vec![bytes(&stream[..split]), bytes(&stream[split..])]));
    fragments.push(stream.iter().map(|b| bytes(&[*b])).collect());
    for (i, input) in fragments.into_iter().enumerate() {
        let (mut dev, trace) = device(input);
        let mut out = Vec::new();
        let result = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0);
        if i == 0 { println!("{name}: success={} output={out:?} error={:?}", result.is_ok(), result.as_ref().err().map(|e| format!("{e:#}"))); }
        assert_eq!(result.is_ok(), expect_success, "{name} fragment {i}: {result:?}");
        if expect_success {
            assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Nak);
        }
        assert_eq!(out.len(), 8);
        if raw.len() == 7 { assert_eq!(out, b"abcdefg\r"); }
        if raw.len() == 9 { assert_eq!(out, b"abcdefgh"); }
        drop(dev);
        assert_eq!(trace.lock().unwrap().writes.len(), 1);
        assert_no_reset(&trace);
    }
}

#[test]
fn short_raw_with_declared_final_ack() {
    run_case("short raw / declared ACK", b"abcdefg", b"\r\n", DECL);
}
#[test]
fn excess_raw_whitespace_with_declared_final_ack() {
    run_case("excess raw / declared ACK", b"abcdefgh\n", b"", DECL);
}
#[test]
fn baseline_undeclared_final_ack_ambiguity() {
    run_case("short raw / undeclared ACK", b"abcdefg", b"\r\n", b"");
}
