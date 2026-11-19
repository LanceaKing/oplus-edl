use oplus_edl_core::result::WriteState;
mod support;
use oplus_edl_core::{
    firehose::WireMode,
    model::Verify,
    program_xml::{Candidate, Operation},
    project::{PackageGuard, extract_project},
    result::Report,
    write::Writes,
};
use support::{ACK, Input, NAK, bytes, disk, gpt_script, storage_device};

// Synthetic format vector for project 23631, not a captured device identity.
// Layout/transform recovered from native extractor 0x180007680 and key 0x180007010.
fn ocdt() -> Vec<u8> {
    let hex = "5444434f0600000010000000340000008293979673925676a252968757567656979687575676569796875756765697968757567656979687575676569796875756765697";
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
fn word(data: &mut [u8], index: usize, value: u32) {
    for (j, b) in value.to_le_bytes().iter().enumerate() {
        let i = index * 4 + j;
        data[16 + i] = (b ^ b"geyixue"[i % 7]).rotate_left(4);
    }
}
fn program(source: &std::path::Path, lun: u8, start: &str) -> Candidate {
    Candidate {
        xml: "fixture.xml".into(),
        xml_node: 1,
        lun,
        slot: 0,
        sector_size: 4096,
        start_sector: start.into(),
        label: Some("boot_a".into()),
        operation: Operation::Program {
            source: source.into(),
            source_bytes: 4096,
            source_offset: 0,
            transfer_bytes: 4096,
            span_bytes: 4096,
            sparse: None,
        },
    }
}
fn script(luns: &[u8], data: &[u8]) -> Vec<Input> {
    let mut input = Vec::new();
    for lun in 0..6 {
        let names = if luns.contains(&lun) {
            vec!["ocdt"]
        } else {
            vec![]
        };
        input.extend(gpt_script(&disk(4096, &names), 4096));
    }
    if luns.len() == 1 {
        let mut block = vec![0; 4096];
        block[..data.len()].copy_from_slice(data);
        input.extend([bytes(ACK), bytes(&block), bytes(ACK)]);
    }
    input
}
fn no_mutation(trace: &support::Trace) {
    let record = trace.lock().unwrap();
    let wire = String::from_utf8_lossy(&record.accepted);
    for cmd in ["<program", "<patch", "<power", "<setbootablestoragedrive"] {
        assert!(!wire.contains(cmd), "unexpected mutation {cmd}");
    }
}

#[test]
fn decoder_requires_bounded_consistent_project_copies() {
    assert_eq!(extract_project(&ocdt()).unwrap(), 23631);
    let mut cases = Vec::new();
    let mut bad = ocdt();
    bad[0] = 0;
    cases.push(bad);
    let mut bad = ocdt();
    bad[8] = 0;
    cases.push(bad);
    let mut bad = ocdt();
    bad[12] = 53;
    cases.push(bad);
    let mut bad = ocdt();
    bad.truncate(67);
    cases.push(bad);
    let mut bad = ocdt();
    word(&mut bad, 2, 0);
    cases.push(bad);
    let mut bad = ocdt();
    word(&mut bad, 1, 23632);
    cases.push(bad);
    let mut bad = ocdt();
    for n in 0..3 {
        word(&mut bad, n, u32::MAX);
    }
    cases.push(bad);
    for bad in cases {
        assert!(extract_project(&bad).is_err());
    }
}

#[test]
fn matching_project_allows_writes_only_after_readonly_preflight() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.img");
    std::fs::write(&path, vec![7; 4096]).unwrap();
    let candidates = vec![program(&path, 0, "64")];
    let guard = PackageGuard::new(23631, &candidates).unwrap();
    let writes = Writes::flash(candidates, 4096, None, false).unwrap();
    let mut input = script(&[3], &ocdt());
    input.extend([bytes(ACK), bytes(ACK)]);
    let (mut dev, trace) = storage_device(input, 4096);
    dev.fh_cfg.storage_type = qdl::types::FirehoseStorageType::Ufs;
    guard
        .validate(&mut dev, WireMode::PreviewBackupGpt)
        .unwrap();
    no_mutation(&trace);
    let mut report = Report::new(Some("flash"), Some("none"));
    writes.execute(&mut dev, Verify::None, &mut report);
    assert_eq!(report.items[0].write, WriteState::Acknowledged);
    let record = trace.lock().unwrap();
    let wire = String::from_utf8_lossy(&record.accepted);
    assert!(wire.contains("special_rw_mode=\"oplus_gptbackup\""));
}

#[test]
fn mismatched_missing_duplicate_and_unreadable_ocdt_never_mutate() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.img");
    std::fs::write(&path, vec![7; 4096]).unwrap();
    for (expected, input, message) in [
        (23632, script(&[3], &ocdt()), "does not match"),
        (23631, script(&[], &[]), "not found"),
        (23631, script(&[2, 3], &[]), "ambiguous"),
        (23631, vec![bytes(NAK)], "read GPT"),
        (23631, script(&[3], &[0; 68]), "invalid OCDT"),
    ] {
        let candidates = vec![program(&path, 0, "64")];
        let guard = PackageGuard::new(expected, &candidates).unwrap();
        let (mut dev, trace) = storage_device(input, 4096);
        dev.fh_cfg.storage_type = qdl::types::FirehoseStorageType::Ufs;
        let error = guard.validate(&mut dev, WireMode::Direct).unwrap_err();
        assert!(format!("{error:#}").contains(message), "{error:#}");
        no_mutation(&trace);
    }
}

#[test]
fn actual_ocdt_overlap_and_out_of_disk_ranges_fail_before_writes() {
    for start in [
        "64",
        "NUM_DISK_SECTORS-192.",
        "NUM_DISK_SECTORS+1.",
        "NUM_DISK_SECTORS-999.",
    ] {
        let mut c = program(std::path::Path::new("unused"), 3, start);
        c.operation = Operation::Patch {
            byte_offset: 0,
            size: 8,
            value: "1".into(),
        };
        let guard = PackageGuard::new(23631, &[c]).unwrap();
        let (mut dev, trace) = storage_device(script(&[3], &ocdt()), 4096);
        dev.fh_cfg.storage_type = qdl::types::FirehoseStorageType::Ufs;
        assert!(guard.validate(&mut dev, WireMode::Direct).is_err());
        no_mutation(&trace);
    }
    let c = program(std::path::Path::new("unused"), 5, "64");
    assert!(PackageGuard::new(23631, &[c]).is_err());
}

#[test]
fn ocdt_payload_cannot_read_past_partition() {
    let mut data = ocdt();
    data[8..10].copy_from_slice(&65520u16.to_le_bytes());
    let guard = PackageGuard::new(23631, &[]).unwrap();
    let (mut dev, trace) = storage_device(script(&[3], &data), 4096);
    dev.fh_cfg.storage_type = qdl::types::FirehoseStorageType::Ufs;
    assert!(
        guard
            .validate(&mut dev, WireMode::Direct)
            .unwrap_err()
            .to_string()
            .contains("exceeds")
    );
    no_mutation(&trace);
}

#[test]
fn extended_ocdt_read_requires_stable_header_and_complete_payload() {
    for changed in [false, true] {
        let original = ocdt();
        let mut header = original[..16].to_vec();
        header[8..10].copy_from_slice(&4096u16.to_le_bytes());
        let mut data = vec![0; 8192];
        data[..16].copy_from_slice(&header);
        data[4096..4096 + 52].copy_from_slice(&original[16..]);
        if changed {
            data[4] ^= 1;
        }
        let mut input = script(&[3], &header);
        input.extend([bytes(ACK), bytes(&data), bytes(ACK)]);
        let guard = PackageGuard::new(23631, &[]).unwrap();
        let (mut dev, trace) = storage_device(input, 4096);
        dev.fh_cfg.storage_type = qdl::types::FirehoseStorageType::Ufs;
        let result = guard.validate(&mut dev, WireMode::Direct);
        if changed {
            assert!(result.unwrap_err().to_string().contains("header changed"));
        } else {
            result.unwrap();
        }
        no_mutation(&trace);
    }
}
