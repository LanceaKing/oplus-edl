use oplus_edl_core::{
    package,
    sparse::{self, Data},
    super_image,
};
use serde_json::json;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[path = "support/package_fixture.rs"]
mod fixtures;
use fixtures::{entry, fixture, package_with_programs};
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
#[test]
fn package_selection_preserves_lun5_ocdt_and_orders_gpt_and_zero_span_userdata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_with_programs(root);
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
    assert!(prepared.candidates.iter().any(|c| matches!(
        c.operation,
        oplus_edl_core::program_xml::Operation::Patch { .. }
    )));
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

#[test]
fn reopened_package_uses_current_payload_without_regenerating_or_rewriting_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_with_programs(root);
    let out = root.join("prepared");
    let generated = package::prepare(root, "123", &out, None, 4096).unwrap();
    let selected_programs = generated.summary.programs.clone();
    let selected_exclusions = generated.summary.exclusions.clone();
    let super_path = generated.summary.super_image.clone();
    drop(generated);

    let mut super_bytes = fs::read(&super_path).unwrap();
    let mut reader = std::io::Cursor::new(&super_bytes);
    let layout = sparse::parse(&mut reader, 4096).unwrap();
    let raw_data_offset = layout
        .chunks
        .iter()
        .find_map(|chunk| match chunk.data {
            Data::Raw { file_offset } if chunk.offset >= 1_048_576 => Some(file_offset),
            _ => None,
        })
        .unwrap();
    super_bytes[raw_data_offset as usize] ^= 0x01;
    fs::write(&super_path, &super_bytes).unwrap();

    let snapshot = |dir: &Path| {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let files_before = snapshot(&out);
    let original_images = snapshot(&root.join("IMAGES"));
    let reopened = package::reopen(root, "123", &out, None, 4096).unwrap();
    assert_eq!(reopened.project, 23631);
    assert_eq!(reopened.summary.programs, selected_programs);
    assert_eq!(reopened.summary.exclusions, selected_exclusions);
    assert!(reopened.candidates.iter().any(|candidate| matches!(
        &candidate.operation, oplus_edl_core::program_xml::Operation::Program { source, .. }
        if source == &super_path
    )));
    assert_eq!(snapshot(&out), files_before);
    assert_eq!(snapshot(&root.join("IMAGES")), original_images);
    assert_eq!(fs::read(super_path).unwrap(), super_bytes);
}

#[test]
fn reopen_rejects_missing_mixed_or_relocated_artifact_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("original");
    fs::create_dir(&root).unwrap();
    package_with_programs(&root);
    let out = temp.path().join("prepared");
    let other = temp.path().join("other-preparation");
    package::prepare(&root, "123", &out, None, 4096).unwrap();
    package::prepare(&root, "123", &other, None, 4096).unwrap();
    let manifest = out.join("selection.json");
    let evidence = fs::read(&manifest).unwrap();

    fs::remove_file(&manifest).unwrap();
    assert!(package::reopen(&root, "123", &out, None, 4096).is_err());
    assert!(!manifest.exists());
    fs::copy(other.join("selection.json"), &manifest).unwrap();
    assert!(package::reopen(&root, "123", &out, None, 4096).is_err());
    fs::write(&manifest, &evidence).unwrap();

    let mut altered: serde_json::Value = serde_json::from_slice(&evidence).unwrap();
    altered.as_object_mut().unwrap().remove("patch_operations");
    fs::write(&manifest, serde_json::to_vec(&altered).unwrap()).unwrap();
    // Earlier outputs carry original patch paths. Their current operations are still validated.
    package::reopen(&root, "123", &out, None, 4096).unwrap();
    altered.as_object_mut().unwrap().remove("raw_size");
    fs::write(&manifest, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(package::reopen(&root, "123", &out, None, 4096).is_err());
    fs::write(&manifest, &evidence).unwrap();

    let program = out.join("rawprogram0.xml");
    let original = fs::read(&program).unwrap();
    fs::copy(other.join("rawprogram0.xml"), &program).unwrap();
    assert!(package::reopen(&root, "123", &out, None, 4096).is_err());
    fs::write(&program, original).unwrap();

    let relocated = temp.path().join("relocated");
    fs::rename(&out, &relocated).unwrap();
    assert!(package::reopen(&root, "123", &relocated, None, 4096).is_err());
    fs::rename(&relocated, &out).unwrap();
    let moved_root = temp.path().join("moved-package");
    fs::rename(&root, &moved_root).unwrap();
    assert!(package::reopen(&moved_root, "123", &out, None, 4096).is_err());
    assert_eq!(fs::read(manifest).unwrap(), evidence);
}

#[test]
fn reopened_xml_and_super_must_match_reconstructed_operations_and_layout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_with_programs(root);
    let out = root.join("prepared");
    let patch = root.join("IMAGES/patch0.xml");
    let patch_xml = r#"<patches><patch filename="DISK" SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="0" start_sector="32" byte_offset="0" size_in_bytes="4" value="1"/></patches>"#;
    fs::write(&patch, patch_xml).unwrap();
    package::prepare(root, "123", &out, None, 4096).unwrap();

    let program = out.join("rawprogram0.xml");
    let original = fs::read_to_string(&program).unwrap();
    let mut ordered = xmltree::Element::parse(original.as_bytes()).unwrap();
    ordered.children.swap(0, 1);
    let mut reordered = Vec::new();
    ordered.write(&mut reordered).unwrap();
    for changed in [
        original
            .replacen("start_sector=\"0\"", "start_sector=\"1\"", 1)
            .into_bytes(),
        original
            .replacen("file_sector_offset=\"0\"", "file_sector_offset=\"1\"", 1)
            .into_bytes(),
        reordered,
    ] {
        assert_ne!(changed, original.as_bytes());
        fs::write(&program, changed).unwrap();
        let error = package::reopen(root, "123", &out, None, 4096)
            .err()
            .unwrap();
        assert!(
            format!("{error:#}").contains("operations or source bindings"),
            "{error:#}"
        );
    }
    fs::write(&program, original).unwrap();
    fs::write(
        &patch,
        patch_xml.replace("byte_offset=\"0\"", "byte_offset=\"4\""),
    )
    .unwrap();
    assert!(package::reopen(root, "123", &out, None, 4096).is_err());
    fs::write(&patch, patch_xml).unwrap();

    let image = out.join("super.img");
    let mut bytes = fs::read(&image).unwrap();
    // A byte in the LP geometry of the first raw chunk.
    bytes[28 + 12 + 4096] ^= 1;
    fs::write(&image, &bytes).unwrap();
    let error = package::reopen(root, "123", &out, None, 4096)
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("LP metadata differs"),
        "{error:#}"
    );
    assert_eq!(fs::read(image).unwrap(), bytes);
}
