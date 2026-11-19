mod support;
use oplus_edl_core::device;
use std::io::{BufRead, ErrorKind, Read};
use support::*;
const RAW_ACK: &[u8] = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
const CONFIG: &[u8] = b"<data><response value=\"ACK\" MinVersionSupported=\"1\" Version=\"1\" MaxPayloadSizeToTargetInBytesSupported=\"4096\" MaxPayloadSizeToTargetInBytes=\"4096\" MaxXMLSizeInBytes=\"4096\"/></data>";
fn packet(cmd: u32, args: &[u32]) -> Vec<u8> {
    [cmd, 8 + 4 * args.len() as u32]
        .into_iter()
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect()
}
fn loader_prefix() -> Vec<u8> {
    [
        packet(1, &[3, 1, 1024, 0, 0, 0, 0, 0, 0, 0]),
        packet(3, &[13, 0, 4]),
        packet(4, &[13, 0]),
        packet(6, &[1]),
        b"<data><log value=\"INFO: End of supported functions\"/></data>".to_vec(),
    ]
    .concat()
}

#[test]
fn preview_vip_request_ends_welcome_without_reading_the_signature_ack() {
    let mut inbound = loader_prefix();
    let end = b"<data><log value=\"INFO: End of supported functions\"/></data>";
    inbound.truncate(inbound.len() - end.len());
    inbound.extend(b"<data><log value=\"INFO: VIP is enabled, receiving the partition info of size 36864\"/></data>");
    inbound.extend(ACK);
    inbound.extend(RAW_ACK);
    inbound.extend(ACK);
    inbound.extend(ACK);
    inbound.extend(CONFIG);
    for chunk in [1, 7, 4096] {
        let (mut dev, _) = storage_device(inbound.chunks(chunk).map(bytes), 512);
        device::initialize_with_digest_signature(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&[0xa5; 512]),
            Some(&[0x5a; 4096]),
            |_| {},
        )
        .unwrap();
    }
}

#[test]
fn preview_digest_profile_preserves_binary_files_and_ack_order() {
    let table = [0xa5; 512];
    let signature = [0x5a; 4096];
    let inbound = [
        loader_prefix(),
        ACK.to_vec(),
        RAW_ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        CONFIG.to_vec(),
    ]
    .concat();
    for chunk in [1, 7, 4096] {
        let (mut dev, trace) = storage_device(inbound.chunks(chunk).map(bytes), 512);
        device::initialize_with_digest_signature(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&table),
            Some(&signature),
            |_| {},
        )
        .unwrap();
        drop(dev);
        assert_no_reset(&trace);
        let trace = trace.lock().unwrap();
        let index = trace.writes.iter().position(|w| w == &table).unwrap();
        assert_eq!(
            trace.writes[index + 1],
            b"<?xml version=\"1.0\"?><data><verify value=\"ping\" EnableVip=\"1\"/></data>"
        );
        assert_eq!(trace.writes[index + 2], signature);
        assert_eq!(
            trace.writes[index + 3],
            b"<?xml version=\"1.0\"?><data><sha256init value=\"ping\"/></data>"
        );
        assert!(
            trace.writes[index + 4]
                .windows(10)
                .any(|w| w == b"<configure")
        );
    }
}

#[test]
fn preview_usb_signature_uses_zero_length_terminator_without_changing_payload() {
    use qdl::types::{QdlBackend, QdlChan};
    let inbound = [
        loader_prefix(),
        ACK.to_vec(),
        RAW_ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        CONFIG.to_vec(),
    ]
    .concat();
    let (mut dev, trace) = storage_device([bytes(&inbound)], 512);
    dev.mut_fh_config().backend = QdlBackend::Usb;
    let signature = [0x5a; 4096];
    device::initialize_with_digest_signature(
        &mut dev,
        &mut [Some(b"abcd".to_vec())],
        Some(&[0xa5; 33144]),
        Some(&signature),
        |_| {},
    )
    .unwrap();
    drop(dev);
    let trace = trace.lock().unwrap();
    let index = trace.writes.iter().position(|w| w == &signature).unwrap();
    assert!(trace.writes[index + 1].is_empty());
    assert!(
        trace.writes[index + 2]
            .windows(11)
            .any(|w| w == b"<sha256init")
    );
}

#[test]
fn preview_digest_rejection_stops_before_the_next_stage() {
    for stage in [0, 1, 2] {
        for bad in [
            NAK,
            b"<data><response value=\"ACK\" rawmode=\"true\"/></data>",
        ] {
            let mut inbound = loader_prefix();
            for i in 0..stage {
                inbound.extend(ACK);
                if i == 0 {
                    inbound.extend(RAW_ACK);
                }
            }
            inbound.extend(bad);
            let (mut dev, trace) = storage_device([bytes(&inbound)], 512);
            let error = device::initialize_with_digest_signature(
                &mut dev,
                &mut [Some(b"abcd".to_vec())],
                Some(&[0xa5; 512]),
                Some(&[0x5a; 4096]),
                |_| {},
            )
            .unwrap_err();
            assert!(format!("{error:#}").contains(match stage {
                0 => "table response",
                1 => "signature response",
                _ => "sha256init response",
            }));
            drop(dev);
            assert_no_reset(&trace);
            let wire = trace.lock().unwrap().accepted.clone();
            assert!(!wire.windows(10).any(|w| w == b"<configure"));
            if stage == 0 {
                assert!(!wire.windows(11).any(|w| w == b"<sha256init"));
            }
        }
    }
}

#[test]
fn preview_verify_requires_raw_ack_before_sending_signature() {
    for bad in [
        NAK,
        ACK,
        b"<data><response value=\"ACK\" rawmode=\"false\"/></data>",
    ] {
        let inbound = [loader_prefix(), ACK.to_vec(), bad.to_vec()].concat();
        let (mut dev, trace) = storage_device([bytes(&inbound)], 512);
        let signature = [0x5a; 4096];
        let error = device::initialize_with_digest_signature(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&[0xa5; 512]),
            Some(&signature),
            |_| {},
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("verify ready response"));
        drop(dev);
        assert!(!trace.lock().unwrap().writes.iter().any(|w| w == &signature));
    }
}

#[test]
fn preview_digest_invalid_material_stops_before_device_io() {
    for (table, signature) in [(None, Some(&[1][..])), (Some(&[1][..]), Some(&[][..]))] {
        let (mut dev, trace) = storage_device([], 512);
        assert!(
            device::initialize_with_digest_signature(
                &mut dev,
                &mut [Some(vec![1])],
                table,
                signature,
                |_| {}
            )
            .is_err()
        );
        assert!(trace.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn signed_digest_is_sent_unchanged_before_configuration() {
    let table = [0xa5; 512];
    let inbound = [loader_prefix(), ACK.to_vec(), CONFIG.to_vec()].concat();
    for chunk in [1, 7, 4096] {
        let (mut dev, trace) = storage_device(inbound.chunks(chunk).map(bytes), 512);
        let mut progress = Vec::new();
        device::initialize_with_progress(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&table),
            |message| progress.push(message.to_owned()),
        )
        .unwrap();
        drop(dev);
        assert_no_reset(&trace);
        let trace = trace.lock().unwrap();
        let digest_index = trace.writes.iter().position(|w| w == &table).unwrap();
        assert!(
            trace.writes[digest_index + 1]
                .windows(10)
                .any(|w| w == b"<configure")
        );
        assert!(
            progress
                .iter()
                .any(|s| s.contains("session authorization not verified"))
        );
        assert!(
            progress
                .last()
                .unwrap()
                .contains("storage access not yet verified")
        );
    }
}

#[test]
fn signed_digest_failure_stops_before_configuration_or_reset() {
    for response in [
        vec![bytes(NAK)],
        vec![bytes(
            b"<data><response value=\"ACK\" rawmode=\"true\"/></data>",
        )],
        vec![bytes(
            b"<data><response value=\"ACK\" rawmode=\"invalid\"/></data>",
        )],
        vec![Input::Error(ErrorKind::TimedOut)],
        vec![
            bytes(b"<data><log value=\"signature rejected\"/></data>"),
            Input::Error(ErrorKind::TimedOut),
        ],
    ] {
        let script = std::iter::once(bytes(&loader_prefix())).chain(response);
        let (mut dev, trace) = storage_device(script, 512);
        let mut progress = Vec::new();
        let error = device::initialize_with_progress(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&[0xa5; 512]),
            |message| progress.push(message.to_owned()),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("signed digest"));
        assert!(!progress.iter().any(|s| s.contains("acknowledged")));
        assert!(!progress.iter().any(|s| s.contains("configuring")));
        drop(dev);
        assert_no_reset(&trace);
        assert!(
            !trace
                .lock()
                .unwrap()
                .accepted
                .windows(10)
                .any(|w| w == b"<configure")
        );
    }
}

#[test]
fn empty_digest_fails_before_sahara_and_startup_errors_keep_the_stage() {
    let (mut dev, trace) = storage_device([], 512);
    assert!(
        device::initialize_with_progress(&mut dev, &mut [Some(vec![1])], Some(&[]), |_| {})
            .is_err()
    );
    assert!(trace.lock().unwrap().writes.is_empty());
    let (mut dev, trace) = storage_device([Input::Error(ErrorKind::TimedOut)], 512);
    let error = device::initialize(&mut dev, &mut [Some(vec![1])]).unwrap_err();
    assert!(format!("{error:#}").contains("Sahara programmer upload"));
    drop(dev);
    assert_no_reset(&trace);
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

#[test]
fn preview_digest_to_program_patch_boot_and_reboot_uses_one_session() {
    use oplus_edl_core::{
        model::Verify,
        program_xml::{Candidate, Operation},
        result::{Report, Status},
        write::Writes,
    };
    let inbound = [
        loader_prefix(),
        ACK.to_vec(),
        RAW_ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        CONFIG.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
        ACK.to_vec(),
    ]
    .concat();
    let (mut dev, trace) = storage_device(inbound.chunks(7).map(bytes), 512);
    device::initialize_with_digest_signature(
        &mut dev,
        &mut [Some(b"abcd".to_vec())],
        Some(&[0xa5; 512]),
        Some(&[0x5a; 4096]),
        |_| {},
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("boot.img");
    std::fs::write(&source, [0x31; 512]).unwrap();
    let program = Candidate {
        xml: "raw.xml".into(),
        xml_node: 1,
        lun: 0,
        slot: 0,
        sector_size: 512,
        start_sector: "64".into(),
        label: Some("boot_a".into()),
        operation: Operation::Program {
            source,
            source_bytes: 512,
            source_offset: 0,
            transfer_bytes: 512,
            span_bytes: 512,
            sparse: None,
        },
    };
    let patch = Candidate {
        xml: "patch.xml".into(),
        xml_node: 1,
        lun: 0,
        slot: 0,
        sector_size: 512,
        start_sector: "0".into(),
        label: None,
        operation: Operation::Patch {
            byte_offset: 16,
            size: 4,
            value: "1".into(),
        },
    };
    let writes = Writes::flash(vec![program, patch], 512, Some(0), true).unwrap();
    writes.preflight(Verify::None).unwrap();
    let mut report = Report::new(Some("flash"), Some("none"));
    writes.execute(&mut dev, Verify::None, &mut report);
    assert_eq!(report.status, Status::Success);
    assert_eq!(report.items.len(), 4);
    let record = trace.lock().unwrap();
    let commands: Vec<_> = record
        .writes
        .iter()
        .filter_map(|w| xmltree::Element::parse(w.as_slice()).ok())
        .flat_map(|e| {
            e.children.into_iter().filter_map(|n| match n {
                xmltree::XMLNode::Element(e) => Some(e),
                _ => None,
            })
        })
        .collect();
    let names: Vec<_> = commands.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "verify",
            "sha256init",
            "configure",
            "program",
            "patch",
            "setbootablestoragedrive",
            "power"
        ]
    );
    assert_eq!(commands[3].attributes["label"], "boot_a");
    assert!(record.writes.iter().any(|w| w == &[0x31; 512]));
}

#[test]
fn preview_boot_preflight_rejects_nonboot_lun_before_mutation() {
    use qdl::types::FirehoseStorageType;
    for (id, valid) in [
        ("0x0", false),
        ("0x1", true),
        ("0x2", true),
        ("garbage", false),
    ] {
        let xml = format!("<data><log value=\"INFO: UFS Boot LUN ID: = {id}\"/></data>");
        let input = [xml.into_bytes(), ACK.to_vec()].concat();
        let (mut ch, trace) = storage_device([bytes(&input)], 4096);
        ch.fh_cfg.storage_type = FirehoseStorageType::Ufs;
        assert_eq!(device::validate_preview_boot_lun(&mut ch, 1).is_ok(), valid);
        let t = trace.lock().unwrap();
        assert_eq!(t.writes.len(), 1);
        assert!(t.writes[0].windows(15).any(|w| w == b"<getstorageinfo"));
    }
}

#[test]
fn digest_idle_timeout_within_grace_keeps_logs_and_continues_on_the_response_refill() {
    let clock = ManualClock::default();
    let mut script = sahara_script();
    script.extend([
        expect_response(Input::Error(ErrorKind::Interrupted)),
        response(b"   \n"),
        response(b"<data><log "),
        response(b"value=\"retained welcome log\"/></data>"),
        on_refill(Access::Response, Input::Error(ErrorKind::TimedOut), {
            let clock = clock.clone();
            move || clock.set(9)
        }),
        response(ACK),
        response(RAW_ACK),
        response(ACK),
        response(ACK),
        response(CONFIG),
    ]);
    let (mut channel, trace) = deadline_channel(script, &clock);
    let mut messages = Vec::new();
    device::initialize_with_digest_signature(
        &mut channel,
        &mut [Some(b"abcd".to_vec())],
        Some(&[1; 32]),
        Some(&[2; 512]),
        |message| messages.push(message.to_owned()),
    )
    .unwrap();
    assert!(
        messages
            .iter()
            .any(|message| message == "retained welcome log")
    );
    assert_eq!(
        wire_commands(&trace)
            .iter()
            .map(|cmd| cmd.name.as_str())
            .collect::<Vec<_>>(),
        ["verify", "sha256init", "configure"]
    );
}
