// Review-only probes; no device enumeration or opening.
mod support;
#[path = "../../cli/src/programfile.rs"]
mod programfile;

use qdl::{firehose_program_storage, firehose_read_storage, firehose_read_welcome};
use qdl::types::{MutationConfirmation, ReadRejected};
use std::io::ErrorKind;
use support::*;

#[test]
fn legacy_erase_input_stops_after_initial_ack() {
    let (mut dev, trace) = device([bytes(ACK)]);
    let error = firehose_program_storage(&mut dev, &mut &[0u8][..], "p", 2, 0, 0, "0").unwrap_err();
    assert_eq!(error.confirmation, MutationConfirmation::Unknown);
    assert_eq!(error.cause.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
    println!("erase: Unknown/UnexpectedEof, one program command, no raw data");
}

#[test]
fn actual_xml_caller_leaves_unaligned_image_partially_written() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/review-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("short.bin"), b"abcdefghi").unwrap();
    let xml = xmltree::Element::parse(&b"<data><program SECTOR_SIZE_IN_BYTES=\"4\" num_partition_sectors=\"3\" physical_partition_number=\"0\" start_sector=\"0\" label=\"p\" filename=\"short.bin\"/></data>"[..]).unwrap();
    let (mut dev, trace) = device([bytes(ACK)]);
    let error = programfile::parse_program_xml(&mut dev, &xml, &dir, &dir, false, false).unwrap_err();
    let failure = error.downcast_ref::<qdl::types::MutationFailure>().unwrap();
    assert_eq!(failure.confirmation, MutationConfirmation::Unknown);
    assert_eq!(failure.cause.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
    let trace = trace.lock().unwrap();
    assert_eq!(trace.writes.len(), 2);
    assert_eq!(trace.writes[1], b"abcdefgh");
    println!("actual parse_program_xml: Unknown/UnexpectedEof after eight raw bytes; final padded sector never sent");
}

#[test]
fn restart_nak_is_exposed_as_safe_read_rejection() {
    for xml in [
        b"<data><response value=\"NAK\" AttemptRestart=\"1\"/></data>".as_slice(),
        b"<data><response value=\"NAK\" rawmode=\"true\"/></data>".as_slice(),
    ] {
        let (mut dev, _) = device([bytes(xml)]);
        let error = firehose_read_storage(&mut dev, &mut Vec::new(), 1, 0, 0, 0).unwrap_err();
        assert!(error.downcast_ref::<ReadRejected>().is_some());
        println!("ReadRejected for {}", String::from_utf8_lossy(xml));
    }
}

#[test]
fn welcome_trailing_whitespace_timeout_is_rejected() {
    let (mut dev, _) = device([bytes(&[LOG, b"\n"].concat()), Input::Error(ErrorKind::TimedOut)]);
    let error = firehose_read_welcome(&mut dev).unwrap_err();
    assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::TimedOut);
    println!("complete welcome XML plus trailing newline returns TimedOut");
}
