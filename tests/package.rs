use oplus_edl::{
    package,
    sparse::{self, Data},
    super_image,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
fn put32(b: &mut [u8], o: usize, n: u32) {
    b[o..o + 4].copy_from_slice(&n.to_le_bytes());
}
fn put64(b: &mut [u8], o: usize, n: u64) {
    b[o..o + 8].copy_from_slice(&n.to_le_bytes());
}
fn sign(b: &mut [u8], o: usize) {
    b[o..o + 32].fill(0);
    let sum = Sha256::digest(&*b);
    b[o..o + 32].copy_from_slice(&sum);
}
fn fixture(root: &Path) -> std::path::PathBuf {
    fs::create_dir(root.join("IMAGES")).unwrap();
    fs::create_dir(root.join("META")).unwrap();
    fs::write(
        root.join("META/build.json"),
        r#"{"ofp":{"product":"23631","build_cfg":{"PRODUCT_VARIANT":"23631"}}}"#,
    )
    .unwrap();
    let mut tables = vec![0u8; 104 + 24 + 48 + 64];
    for (offset, label) in [(0, b"system_a"), (52, b"system_b")] {
        tables[offset..offset + 8].copy_from_slice(label);
        put32(&mut tables, offset + 36, 1);
    }
    put32(&mut tables, 44, 1);
    put32(&mut tables, 92, 1);
    put64(&mut tables, 104, 24);
    put64(&mut tables, 116, 2048);
    tables[128..135].copy_from_slice(b"default");
    put64(&mut tables, 176, 2048);
    put32(&mut tables, 184, 1048576);
    put64(&mut tables, 192, 2097152);
    tables[200..205].copy_from_slice(b"super");
    let mut header = vec![0u8; 128];
    put32(&mut header, 0, 0x414c5030);
    put32(&mut header, 4, 10);
    put32(&mut header, 8, 128);
    put32(&mut header, 44, tables.len() as u32);
    header[48..80].copy_from_slice(&Sha256::digest(&tables));
    for (i, (offset, count, stride)) in [(0, 2, 52), (104, 1, 24), (128, 1, 48), (176, 1, 64)]
        .iter()
        .enumerate()
    {
        let at = 80 + i * 12;
        put32(&mut header, at, *offset);
        put32(&mut header, at + 4, *count);
        put32(&mut header, at + 8, *stride);
    }
    sign(&mut header, 12);
    let mut geometry = vec![0u8; 52];
    put32(&mut geometry, 0, 0x616c4467);
    put32(&mut geometry, 4, 52);
    put32(&mut geometry, 40, 65536);
    put32(&mut geometry, 44, 3);
    put32(&mut geometry, 48, 4096);
    sign(&mut geometry, 8);
    geometry.resize(4096, 0);
    geometry.extend(header);
    geometry.extend(tables);
    fs::write(root.join("IMAGES/super_meta.raw"), geometry).unwrap();
    let mut sparse = vec![0u8; 28];
    put32(&mut sparse, 0, sparse::MAGIC);
    sparse[4] = 1;
    sparse[8] = 28;
    sparse[10] = 12;
    put32(&mut sparse, 12, 4096);
    put32(&mut sparse, 16, 3);
    put32(&mut sparse, 20, 3);
    for (kind, payload) in [
        (0xcac1u16, vec![0xa5; 4096]),
        (0xcac2, vec![1, 2, 3, 4]),
        (0xcac3, vec![]),
    ] {
        sparse.extend(kind.to_le_bytes());
        sparse.extend(0u16.to_le_bytes());
        sparse.extend(1u32.to_le_bytes());
        sparse.extend((12 + payload.len() as u32).to_le_bytes());
        sparse.extend(payload);
    }
    fs::write(root.join("IMAGES/system.img"), sparse).unwrap();
    let path = root.join("META/super_def.123.json");
    fs::write(&path,serde_json::to_vec(&json!({"super_meta":{"path":"IMAGES/super_meta.raw"},"block_devices":[{"name":"super","size":"2097152","block_size":"4096","alignment":"1048576"}],"groups":[{"name":"default"}],"partitions":[{"name":"system_a","size":"12288","path":"IMAGES/system.img","group_name":"default"},{"name":"system_b","group_name":"default"}]})).unwrap()).unwrap();
    path
}
#[test]
fn super_preserves_reference_slots_and_relocates_sparse_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let def = fixture(root);
    let out = root.join("super.img");
    super_image::build(root, &def, &out).unwrap();
    let mut f = fs::File::open(&out).unwrap();
    let image = sparse::parse(&mut f, 4096).unwrap();
    assert_eq!(image.expanded_bytes, 2097152);
    let mut expanded = vec![0u8; 2097152];
    for c in image.chunks {
        let slice = &mut expanded[c.offset as usize..(c.offset + c.bytes) as usize];
        match c.data {
            Data::Raw { file_offset } => {
                f.seek(SeekFrom::Start(file_offset)).unwrap();
                f.read_exact(slice).unwrap();
            }
            Data::Fill(v) => {
                for x in slice.as_chunks_mut::<4>().0 {
                    x.copy_from_slice(&v);
                }
            }
            Data::DontCare => {}
        }
    }
    let meta = fs::read(root.join("IMAGES/super_meta.raw")).unwrap();
    assert_eq!(&expanded[4096..8192], &meta[..4096]);
    assert_eq!(&expanded[8192..12288], &meta[..4096]);
    for i in 0..6 {
        assert_eq!(
            &expanded[12288 + i * 65536..12288 + i * 65536 + meta.len() - 4096],
            &meta[4096..]
        );
    }
    assert!(expanded[1048576..1052672].iter().all(|b| *b == 0xa5));
    assert_eq!(&expanded[1052672..1052676], &[1, 2, 3, 4]);
    assert!(expanded[1056768..].iter().all(|b| *b == 0));
    let before = fs::read(&out).unwrap();
    assert!(super_image::build(root, &def, &out).is_err());
    assert_eq!(fs::read(out).unwrap(), before);
}
#[test]
fn super_rejects_corrupt_reference_and_wrong_image_size_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let def = fixture(root);
    let out = root.join("super.img");
    let p = root.join("IMAGES/super_meta.raw");
    let mut b = fs::read(&p).unwrap();
    b[100 + 4096] ^= 1;
    fs::write(&p, &b).unwrap();
    assert!(
        super_image::build(root, &def, &out)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    assert!(!out.exists());
    b[100 + 4096] ^= 1;
    fs::write(&p, b).unwrap();
    fs::write(root.join("IMAGES/system.img"), [1; 4096]).unwrap();
    assert!(
        super_image::build(root, &def, &out)
            .unwrap_err()
            .to_string()
            .contains("size differs")
    );
    assert!(!out.exists());
}
fn entry(lun: u8, label: &str, file: &str, sectors: u64, sparse: bool) -> String {
    format!(
        r#"<program SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="{lun}" start_sector="0" num_partition_sectors="{sectors}" file_sector_offset="0" sparse="{sparse}" label="{label}" filename="{file}"/>"#
    )
}
#[test]
fn package_selection_preserves_lun5_ocdt_and_orders_gpt_and_zero_span_userdata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    fs::write(root.join("IMAGES/gpt.bin"), [0; 4096]).unwrap();
    for lun in 0..=5 {
        let mut body = entry(lun, "BackupGPT", "gpt.bin", 1, false);
        if lun == 0 {
            body += &entry(lun, "super", "super.img", 512, true);
            body += &entry(lun, "userdata", "system.img", 0, true);
        }
        body += &entry(lun, "ocdt", "missing.bin", 1, false);
        body += &entry(lun, "secdata", "sec.dat", 1, false);
        body += &entry(lun, "PrimaryGPT", "gpt.bin", 1, false);
        fs::write(
            root.join(format!("IMAGES/rawprogram{lun}.xml")),
            format!("<data>{body}</data>"),
        )
        .unwrap();
        fs::write(root.join(format!("IMAGES/patch{lun}.xml")), "<patches/>").unwrap();
    }
    let config = root.join("flash_config.json");
    fs::write(
        &config,
        r#"{"excludedPartitionsForRead":["userdata"],"excludedPartitionsForWrite":[]}"#,
    )
    .unwrap();
    let p = package::prepare(root, "123", &root.join("prepared"), Some(&config), 4096).unwrap();
    let labels: Vec<_> = p
        .candidates
        .iter()
        .map(|c| c.label.as_deref().unwrap())
        .collect();
    assert_eq!(
        &labels[..4],
        &["PrimaryGPT", "super", "userdata", "BackupGPT"]
    );
    assert_eq!(labels.len(), 12);
    assert!(p.candidates.iter().all(|c| c.lun < 5));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(p.manifest).unwrap()).unwrap();
    assert_eq!(manifest["program_order"][1]["filename"], "super.img");
    assert!(package::prepare(root, "123", &root.join("prepared"), None, 4096).is_err());
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(["--json", "flash", "--package"])
        .arg(root)
        .args(["--nv-id", "123", "--prepare-output"])
        .arg(root.join("cli-output"))
        .arg("--prepare-only")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(report["target"].is_null());
    assert_eq!(report["status"], "success");
    fs::write(&config, r#"{"excludedPartitionsForWrite":["userdata"]}"#).unwrap();
    let filtered =
        package::prepare(root, "123", &root.join("filtered"), Some(&config), 4096).unwrap();
    assert!(
        !filtered
            .candidates
            .iter()
            .any(|c| c.label.as_deref() == Some("userdata"))
    );
}

#[test]
fn package_cli_requires_variant_and_does_not_mix_explicit_xml() {
    use clap::Parser;
    use oplus_edl::cli::Cli;
    assert!(Cli::try_parse_from(["oplus-edl", "flash", "--package", "p"]).is_err());
    assert!(Cli::try_parse_from(["oplus-edl", "flash", "--prepare-only"]).is_err());
    assert!(
        Cli::try_parse_from([
            "oplus-edl",
            "flash",
            "--package",
            "p",
            "--nv-id",
            "123",
            "--prepare-output",
            "o",
            "--program",
            "x"
        ])
        .is_err()
    );
}

fn package_fixture(root: &Path, super_entries: &str, patch: &str) {
    fixture(root);
    fs::write(root.join("IMAGES/gpt.bin"), [0; 4096]).unwrap();
    for lun in 0..=5 {
        let body = entry(lun, "PrimaryGPT", "gpt.bin", 6, false)
            + &entry(lun, "BackupGPT", "gpt.bin", 5, false)
                .replace("start_sector=\"0\"", "start_sector=\"NUM_DISK_SECTORS-5.\"")
            + if lun == 0 { super_entries } else { "" };
        fs::write(
            root.join(format!("IMAGES/rawprogram{lun}.xml")),
            format!("<data>{body}</data>"),
        )
        .unwrap();
        fs::write(
            root.join(format!("IMAGES/patch{lun}.xml")),
            format!("<patches>{}</patches>", if lun == 0 { patch } else { "" }),
        )
        .unwrap();
    }
}

fn disk_patch(start: &str, offset: u64) -> String {
    format!(
        r#"<patch SECTOR_SIZE_IN_BYTES="4096" filename="DISK" physical_partition_number="0" start_sector="{start}" byte_offset="{offset}" size_in_bytes="8" value="1"/>"#
    )
}

#[test]
fn excluded_gpt_cannot_be_modified_by_disk_patches() {
    for (label, start, offset) in [
        ("PrimaryGPT", "2", 1704),
        ("PrimaryGPT", "1", 48),
        ("BackupGPT", "NUM_DISK_SECTORS-1.", 24),
        // The address cannot be proved disjoint without device capacity.
        ("PrimaryGPT", "NUM_DISK_SECTORS-1.", 24),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        package_fixture(
            root,
            &entry(0, "super", "super.img", 512, true),
            &disk_patch(start, offset),
        );
        let config = root.join("config.json");
        fs::write(
            &config,
            serde_json::to_vec(&json!({"excludedPartitionsForWrite":[label]})).unwrap(),
        )
        .unwrap();
        let error = package::prepare(root, "123", &root.join("out"), Some(&config), 4096)
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("cannot prove DISK patch avoids"),
            "{error:#}"
        );
        assert!(!root.join("out/selection.json").exists());
    }
}

#[test]
fn excluded_numeric_range_allows_provably_disjoint_disk_patch() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_fixture(
        root,
        &entry(0, "super", "super.img", 512, true),
        &disk_patch("10", 0),
    );
    let config = root.join("config.json");
    fs::write(&config, r#"{"excludedPartitionsForWrite":["PrimaryGPT"]}"#).unwrap();
    let prepared = package::prepare(root, "123", &root.join("out"), Some(&config), 4096).unwrap();
    assert_eq!(prepared.project, 23631);
    assert!(
        prepared
            .candidates
            .iter()
            .any(|c| matches!(c.operation, oplus_edl::program_xml::Operation::Patch { .. }))
    );
}

#[test]
fn arbitrary_excluded_partitions_and_unknown_spans_fail_closed() {
    for sectors in [0, 2] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        package_fixture(
            root,
            &(entry(0, "super", "super.img", 512, true)
                + &entry(0, "private", "missing.bin", sectors, false)
                    .replace("start_sector=\"0\"", "start_sector=\"20\"")),
            &disk_patch("20", 0),
        );
        let config = root.join("config.json");
        fs::write(&config, r#"{"excludedPartitionsForWrite":["private"]}"#).unwrap();
        let error = package::prepare(root, "123", &root.join("out"), Some(&config), 4096)
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("cannot prove DISK patch avoids"),
            "{error:#}"
        );
    }
}

#[test]
fn package_requires_one_executable_super_with_exact_span() {
    let valid = entry(0, "super", "super.img", 512, true);
    for body in [
        entry(0, "super", "super.img", 0, true),
        valid.clone() + &valid,
        entry(0, "super", "super.img", 513, true),
        entry(0, "super", "super.img", 512, false),
        String::new(),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        package_fixture(root, &body, "");
        assert!(package::prepare(root, "123", &root.join("out"), None, 4096).is_err());
        assert!(!root.join("out/selection.json").exists());
    }
}

#[test]
fn package_project_identity_is_required_and_consistent() {
    for metadata in [
        None,
        Some(r#"{}"#),
        Some(r#"{"ofp":{"product":23631}}"#),
        Some(r#"{"ofp":{"product":""}}"#),
        Some(r#"{"ofp":{"product":"0"}}"#),
        Some(r#"{"ofp":{"product":"23631|23632"}}"#),
        Some(r#"{"ofp":{"product":"4294967296"}}"#),
        Some(r#"{"ofp":{"product":"23631","build_cfg":{"PRODUCT_VARIANT":"23632"}}}"#),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        package_fixture(root, &entry(0, "super", "super.img", 512, true), "");
        let path = root.join("META/build.json");
        if let Some(metadata) = metadata {
            fs::write(path, metadata).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        assert!(package::prepare(root, "123", &root.join("out"), None, 4096).is_err());
        assert!(!root.join("out").exists());
    }
}
