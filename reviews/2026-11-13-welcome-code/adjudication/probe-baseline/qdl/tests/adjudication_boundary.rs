// SPDX-License-Identifier: BSD-3-Clause
mod support;
use qdl::{firehose_read, firehose_read_storage, parsers::firehose_parser_ack_nak, types::FirehoseStatus};
use support::*;

const DECL: &[u8] = b"<?xml version=\"1.0\"?>";

#[test]
fn length_boundary_and_declared_response_matrix() {
    let baseline = std::env::var_os("ADJ_BASELINE").is_some();
    for (label, after_initial_ack, current_ok, baseline_ok, expected_out) in [
        ("ambiguous_declared", [b"1234567\r\n".as_slice(), DECL, ACK].concat(), true, false, b"1234567\r".as_slice()),
        ("ambiguous_undeclared", [b"1234567\r\n".as_slice(), ACK].concat(), true, true, b"1234567\r".as_slice()),
        ("eight_bytes_no_gap", [b"1234567\r".as_slice(), DECL, ACK].concat(), true, true, b"1234567\r".as_slice()),
        ("seven_bytes_no_gap", [b"1234567".as_slice(), DECL, ACK].concat(), false, false, b"1234567<".as_slice()),
        ("eight_whitespace_bytes", [b" \t\r\n \t\r\n".as_slice(), DECL, ACK].concat(), true, true, b" \t\r\n \t\r\n".as_slice()),
    ] {
        let stream = [ACK, &after_initial_ack, NAK].concat();
        let mut fragments = vec![vec![bytes(&stream)]];
        fragments.extend((1..stream.len()).map(|split| vec![bytes(&stream[..split]), bytes(&stream[split..])]));
        fragments.push(stream.iter().map(|byte| bytes(&[*byte])).collect());
        let expected_ok = if baseline { baseline_ok } else { current_ok };
        let count = fragments.len();
        for (split, input) in fragments.into_iter().enumerate() {
            let (mut dev, trace) = device(input);
            let mut out = Vec::new();
            let result = firehose_read_storage(&mut dev, &mut out, 2, 0, 0, 0);
            assert_eq!(out, expected_out, "{label} fragmentation={split}");
            assert_eq!(result.is_ok(), expected_ok, "{label} fragmentation={split} result={result:?}");
            if expected_ok {
                assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Nak);
            }
            assert_eq!(trace.lock().unwrap().writes.len(), 1);
            assert_no_reset(&trace);
        }
        println!("{label}: success={expected_ok}; output={expected_out:?}; {count} fragmentations");
    }
}
