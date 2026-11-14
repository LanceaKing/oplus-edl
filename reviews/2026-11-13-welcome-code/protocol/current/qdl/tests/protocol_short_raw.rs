// Independent reproduction of the coordinator-supplied byte-stream counterexample.
mod support;

use qdl::firehose_read_storage;
use support::*;

const DECL: &[u8] = b"<?xml version=\"1.0\"?>";

#[test]
fn protocol_short_raw_declared_final_ack_must_fail() {
    let stream = [ACK, b"1234567\r\n", DECL, ACK].concat();
    for split in 1..stream.len() {
        let (mut dev, trace) = device([bytes(&stream[..split]), bytes(&stream[split..])]);
        let mut out = Vec::new();
        let result = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0);
        println!("split={split}; result={result:?}; out={out:?}");
        assert_eq!(out, b"1234567\r");
        assert_eq!(trace.lock().unwrap().writes.len(), 1);
        assert!(result.is_err(), "seven source bytes plus CRLF must not complete an eight-byte read");
    }
}

#[test]
fn protocol_short_raw_undeclared_final_ack_is_preexisting_ambiguity() {
    let (mut dev, _) = device([bytes(&[ACK, b"1234567\r\n", ACK].concat())]);
    let mut out = Vec::new();
    firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0).unwrap();
    assert_eq!(out, b"1234567\r");
}
