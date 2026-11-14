mod support;
use oplus_edl::device;
use std::io::{BufRead, ErrorKind, Read};
use support::*;
const CONFIG: &[u8] = b"<data><response value=\"ACK\" MinVersionSupported=\"1\" Version=\"1\" MaxPayloadSizeToTargetInBytesSupported=\"4096\" MaxPayloadSizeToTargetInBytes=\"4096\" MaxXMLSizeInBytes=\"4096\"/></data>";
fn packet(cmd: u32, args: &[u32]) -> Vec<u8> {
    [cmd, 8 + 4 * args.len() as u32]
        .into_iter()
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect()
}
#[test]
fn initialize_real_protocol_with_fragmented_script_only() {
    let inbound = [
        packet(1, &[2, 1, 4096, 0, 0, 0, 0, 0, 0, 0]),
        packet(3, &[13, 0, 4]),
        packet(4, &[13, 0]),
        packet(6, &[1]),
        b"<data><log value=\"INFO: End of supported functions\"/></data>".to_vec(),
        CONFIG.to_vec(),
    ]
    .concat();
    for chunk in [1, 7, 4096] {
        let (mut dev, trace) = storage_device(inbound.chunks(chunk).map(bytes), 512);
        device::initialize(&mut dev, &mut [Some(b"abcd".to_vec())]).unwrap();
        drop(dev);
        assert_no_reset(&trace);
        let wire = String::from_utf8_lossy(&trace.lock().unwrap().accepted).into_owned();
        assert_eq!(wire.matches("<configure").count(), 1);
        assert!(wire.contains("SkipWrite=\"0\""));
    }
}
#[test]
fn configuration_rejects_nak_conflict_and_malformed_sector_without_reset() {
    let conflict = String::from_utf8(CONFIG.to_vec()).unwrap().replace(
        "value=\"ACK\"",
        "value=\"ACK\" SECTOR_SIZE_IN_BYTES=\"4096\"",
    );
    let malformed = conflict.replace(
        "SECTOR_SIZE_IN_BYTES=\"4096\"",
        "SECTOR_SIZE_IN_BYTES=\"bad\"",
    );
    for response in [NAK, conflict.as_bytes(), malformed.as_bytes()] {
        let (mut dev, trace) = storage_device([bytes(response)], 512);
        assert!(device::configure(&mut dev).is_err());
        drop(dev);
        assert_no_reset(&trace);
        assert_eq!(trace.lock().unwrap().writes.len(), 1);
    }
}
#[test]
fn handshake_failure_does_not_configure_or_reset() {
    let (mut dev, trace) = storage_device([Input::Error(ErrorKind::TimedOut)], 512);
    assert!(device::initialize(&mut dev, &mut [Some(vec![1])]).is_err());
    drop(dev);
    assert!(trace.lock().unwrap().writes.is_empty());
}
#[test]
fn scripted_read_and_bufread_share_position() {
    let mut io = ScriptedIo::new([bytes(b"abc"), Input::Eof]);
    assert_eq!(io.fill_buf().unwrap(), b"abc");
    assert_eq!(io.fill_buf().unwrap(), b"abc");
    io.consume(1);
    let mut out = [0; 1];
    io.read_exact(&mut out).unwrap();
    assert_eq!(&out, b"b");
    assert_eq!(io.fill_buf().unwrap(), b"c");
    io.consume(1);
    assert!(io.fill_buf().unwrap().is_empty());
}

#[test]
fn loader_rejects_unexpected_sahara_workflows_without_any_response() {
    for unexpected in [
        packet(0x10, &[0, 0, u32::MAX, u32::MAX]),
        packet(0x0b, &[]),
        packet(0x0e, &[1, u32::MAX]),
        packet(0x08, &[]),
    ] {
        let (mut dev, trace) = storage_device([bytes(&unexpected)], 512);
        assert!(device::initialize(&mut dev, &mut [Some(vec![1])]).is_err());
        drop(dev);
        assert!(trace.lock().unwrap().writes.is_empty());
        // No additional scripted reads exist: a debug-table read, configure,
        // reset or other follow-up would also fail this test immediately.
    }
}
