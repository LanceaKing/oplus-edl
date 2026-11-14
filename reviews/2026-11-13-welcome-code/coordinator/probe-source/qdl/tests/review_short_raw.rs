// SPDX-License-Identifier: BSD-3-Clause
mod support;
use qdl::firehose_read_storage;
use support::*;
#[test]
fn short_raw_data_before_declared_final_ack_must_fail() {
    let final_ack = b"<?xml version=\"1.0\"?><data><response value=\"ACK\"/></data>";
    let (mut dev, trace) = device([bytes(ACK), bytes(&[&b"1234567\r\n"[..], final_ack].concat())]);
    let mut out = Vec::new();
    let result = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0);
    println!("result={result:?}; output={out:?}");
    assert_eq!(out, b"1234567\r");
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
    assert!(result.is_err(), "short raw data was accepted with framing CR substituted into output");
}
