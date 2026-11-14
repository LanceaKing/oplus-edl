mod support;
use qdl::{firehose_read, parsers::firehose_parser_ack_nak, types::FirehoseStatus};
use support::*;
#[test]
fn upstream_sentinel_trailing_whitespace_failure_is_preexisting() {
    let end = b"<data><log value=\"INFO: End of supported functions\"/></data>\r\n";
    let declared_ack = b"<?xml version=\"1.0\"?><data><response value=\"ACK\"/></data>";
    let (mut dev, _) = device([bytes(end), bytes(declared_ack)]);
    assert_eq!(firehose_read(&mut dev, firehose_parser_ack_nak).unwrap(), FirehoseStatus::Ack);
    let error = firehose_read(&mut dev, firehose_parser_ack_nak).unwrap_err();
    println!("upstream error: {error:#}");
    assert!(error.to_string().contains("only valid at the beginning of the document"));
}
