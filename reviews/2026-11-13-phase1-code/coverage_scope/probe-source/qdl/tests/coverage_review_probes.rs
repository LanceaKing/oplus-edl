mod support;

#[path = "../../cli/src/programfile.rs"]
mod programfile;

use qdl::{firehose_program_storage, firehose_read_welcome};
use qdl::types::{MutationConfirmation, MutationFailure};
use std::io::ErrorKind;
use support::*;

#[test]
fn existing_erase_source_fails_after_program_is_accepted() {
    // Exactly the source supplied by cli/src/main.rs Command::Erase.
    let (mut dev, trace) = device([bytes(ACK)]);
    let error = firehose_program_storage(&mut dev, &mut &[0u8][..], "p", 2, 0, 0, "0")
        .unwrap_err();
    assert_eq!(error.confirmation, MutationConfirmation::Unknown);
    assert_eq!(error.cause.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
    println!("erase caller: {error:#}; only program command sent");
}

#[test]
fn actual_xml_caller_omits_final_sector_padding() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/review-fixtures");
    std::fs::create_dir_all(&fixture_dir).unwrap();
    std::fs::write(fixture_dir.join("short.bin"), b"abcdefghi").unwrap();
    let xml = xmltree::Element::parse(&b"<data><program SECTOR_SIZE_IN_BYTES=\"4\" num_partition_sectors=\"3\" physical_partition_number=\"0\" start_sector=\"0\" label=\"p\" filename=\"short.bin\"/></data>"[..]).unwrap();
    let (mut dev, trace) = device([bytes(ACK)]);
    let error = programfile::parse_program_xml(&mut dev, &xml, &fixture_dir, &fixture_dir, false, false)
        .unwrap_err();
    let mutation = error.downcast_ref::<MutationFailure>().unwrap();
    assert_eq!(mutation.confirmation, MutationConfirmation::Unknown);
    assert_eq!(mutation.cause.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::UnexpectedEof);
    let record = trace.lock().unwrap();
    assert_eq!(record.writes.len(), 2);
    assert_eq!(record.writes[1], b"abcdefgh");
    println!("actual parse_program_xml caller: {error:#}; sent 8 of 12 promised bytes");
}

#[test]
fn completed_welcome_document_with_trailing_whitespace_should_end_on_idle() {
    let (mut dev, _) = device([bytes(&[LOG, b"\r\n"].concat()), Input::Error(ErrorKind::TimedOut)]);
    let result = firehose_read_welcome(&mut dev);
    assert!(result.is_ok(), "valid completed XML followed by whitespace: {result:?}");
}
