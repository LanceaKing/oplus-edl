// Review-only offline probes; production source is unchanged.
mod support;

use qdl::{firehose_program_storage, firehose_read_welcome};
use std::io::ErrorKind;
use support::*;

#[test]
fn welcome_trailing_xml_whitespace_is_a_document_boundary() {
    for tail in [b"\n".as_slice(), b"\r\n", b" \t\n"] {
        let (mut dev, _) = device([
            bytes(&[LOG, tail].concat()),
            Input::Error(ErrorKind::TimedOut),
        ]);
        let result = firehose_read_welcome(&mut dev);
        assert!(result.is_ok(), "Complete welcome XML plus {tail:?}: {result:?}");
    }
}

#[test]
fn existing_cli_erase_source_remains_usable() {
    let (mut dev, trace) = device([bytes(ACK), bytes(ACK)]);
    // cli/src/main.rs:343 supplies this one-byte zero source to erase a partition.
    let result = firehose_program_storage(&mut dev, &mut &[0u8][..], "erase", 2, 0, 0, "0");
    assert!(result.is_ok(), "Existing erase call: {result:?}; trace={:?}", trace.lock().unwrap());
}

#[test]
fn existing_cli_rounded_image_source_remains_usable() {
    let (mut dev, trace) = device([bytes(ACK), bytes(ACK)]);
    // Write and OverwriteStorage round 5 bytes to two 4-byte sectors, then pass the file unchanged.
    let result = firehose_program_storage(&mut dev, &mut &b"abcde"[..], "write", 2, 0, 0, "0");
    assert!(result.is_ok(), "Existing rounded image call: {result:?}; trace={:?}", trace.lock().unwrap());
}
