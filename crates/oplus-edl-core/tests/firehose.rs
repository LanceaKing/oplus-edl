mod support;
use oplus_edl_core::firehose;
use std::io::Cursor;
use support::*;
#[test]
fn program_sends_escaped_label_and_exact_payload() {
    let (mut channel, trace) = device([
        bytes(b"<data><response value=\"ACK\" rawmode=\"true\"/></data>"),
        bytes(b"<data><response value=\"ACK\" rawmode=\"false\"/></data>"),
    ]);
    channel.fh_cfg.storage_sector_size = 512;
    channel.fh_cfg.send_buffer_size = 512;
    channel.fh_cfg.recv_buffer_size = 512;
    firehose::program(
        &mut channel,
        &mut Cursor::new(vec![0xab; 512]),
        "a<&\"b",
        1,
        0,
        4,
        "64",
    )
    .unwrap();
    let t = trace.lock().unwrap();
    let xml = xmltree::Element::parse(t.writes[0].as_slice()).unwrap();
    let p = xml.get_child("program").unwrap();
    assert_eq!(p.attributes["label"], "a<&\"b");
    assert_eq!(p.attributes["physical_partition_number"], "4");
    assert_eq!(t.writes[1], vec![0xab; 512]);
}
#[test]
fn wrong_mode_ack_never_sends_payload() {
    let (mut channel, trace) = device([bytes(
        b"<data><response value=\"ACK\" rawmode=\"false\"/></data>",
    )]);
    assert!(
        firehose::program(
            &mut channel,
            &mut Cursor::new(vec![0xab; 512]),
            "boot",
            1,
            0,
            4,
            "64"
        )
        .is_err()
    );
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
}
#[test]
fn preview_mode_is_probed_and_preserves_target_range() {
    let ack = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
    let inbound = [
        ack.to_vec(),
        vec![0; 5 * 512],
        ACK.to_vec(),
        ack.to_vec(),
        ACK.to_vec(),
    ]
    .concat();
    let (mut ch, trace) = storage_device(inbound.chunks(7).map(bytes), 512);
    let mode = firehose::probe_preview_mode(&mut ch).unwrap();
    firehose::program_with_mode(
        &mut ch,
        &mut Cursor::new(vec![0xab; 512]),
        "super",
        1,
        0,
        4,
        "99052",
        mode,
    )
    .unwrap();
    let t = trace.lock().unwrap();
    let xml = xmltree::Element::parse(t.writes[1].as_slice()).unwrap();
    let p = xml.get_child("program").unwrap();
    assert_eq!(p.attributes["label"], "BackupGPT");
    assert_eq!(p.attributes["filename"], "gpt_backup4.bin");
    assert_eq!(p.attributes["special_rw_mode"], "oplus_gptbackup");
    assert_eq!(p.attributes["start_sector"], "99052");
    assert_eq!(p.attributes["physical_partition_number"], "4");
    assert_eq!(p.attributes["num_partition_sectors"], "1");
    assert_eq!(t.writes[2], vec![0xab; 512]);
}
#[test]
fn preview_probe_refusal_does_not_send_program() {
    let (mut ch, trace) = storage_device([bytes(NAK)], 512);
    assert!(firehose::probe_preview_mode(&mut ch).is_err());
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
}

#[test]
fn read_accepts_legacy_modes_and_rejects_explicit_mismatch() {
    for raw in [None, Some("true")] {
        for final_mode in [None, Some("false")] {
            let ack = |mode: Option<&str>| match mode {
                None => ACK.to_vec(),
                Some(mode) => format!("<data><response value=\"ACK\" rawmode=\"{mode}\"/></data>")
                    .into_bytes(),
            };
            let input = [ack(raw), vec![0x5a; 512], ack(final_mode)].concat();
            let (mut ch, _) = storage_device(input.chunks(7).map(bytes), 512);
            let mut output = vec![];
            firehose::read_storage(&mut ch, &mut output, 1, 0, 4, 64).unwrap();
            assert_eq!(output, vec![0x5a; 512]);
        }
    }
    for mode in ["false", "invalid"] {
        let input = format!("<data><response value=\"ACK\" rawmode=\"{mode}\"/></data>");
        let (mut ch, _) = storage_device([bytes(input.as_bytes())], 512);
        let mut output = vec![];
        assert!(firehose::read_storage(&mut ch, &mut output, 1, 0, 4, 64).is_err());
        assert!(output.is_empty());
    }
}

#[test]
fn preview_read_mode_preserves_target_geometry() {
    let inbound = [ACK.to_vec(), vec![0x5a; 1024], ACK.to_vec()].concat();
    let (mut ch, trace) = storage_device([bytes(&inbound)], 512);
    let mut out = vec![];
    firehose::read_storage_with_mode(
        &mut ch,
        &mut out,
        2,
        3,
        4,
        99052,
        firehose::WireMode::PreviewBackupGpt,
    )
    .unwrap();
    assert_eq!(out, vec![0x5a; 1024]);
    let trace = trace.lock().unwrap();
    let xml = xmltree::Element::parse(trace.writes[0].as_slice()).unwrap();
    let read = xml.get_child("read").unwrap();
    for (key, expected) in [
        ("label", "BackupGPT"),
        ("filename", "gpt_backup4.bin"),
        ("special_rw_mode", "oplus_gptbackup"),
        ("num_partition_sectors", "2"),
        ("physical_partition_number", "4"),
        ("slot", "3"),
        ("start_sector", "99052"),
    ] {
        assert_eq!(read.attributes[key], expected);
    }
}
