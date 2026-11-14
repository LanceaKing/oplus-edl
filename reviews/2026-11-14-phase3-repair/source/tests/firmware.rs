use oplus_edl::{
    program_xml::{self, Operation},
    sparse::{self, Data},
};
use std::{fs, io::Cursor, path::Path};
fn sparse_bytes() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(sparse::MAGIC.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    b.extend(28u16.to_le_bytes());
    b.extend(12u16.to_le_bytes());
    for n in [512u32, 3, 3, 0] {
        b.extend(n.to_le_bytes());
    }
    for (kind, payload) in [
        (0xcac1u16, vec![9; 512]),
        (0xcac3, vec![]),
        (0xcac2, vec![1, 2, 3, 4]),
    ] {
        b.extend(kind.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend((12 + payload.len() as u32).to_le_bytes());
        b.extend(payload);
    }
    b
}
fn program(filename: &str, extra: &str) -> String {
    format!(r#"<program filename="{filename}" label="boot" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="3" {extra}/ >"#).replace("/ >","/>")
}
fn patch(filename: &str) -> String {
    format!(
        r#"<patch filename="{filename}" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="NUM_DISK_SECTORS-1." byte_offset="16" size_in_bytes="4" value="CRC32(0,512)"/>"#
    )
}
fn xml(root: &Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = root.join(name);
    fs::write(&path, format!("<data>{body}</data>")).unwrap();
    path
}
#[test]
fn sparse_ranges_retain_gaps_and_fill_pattern_without_expansion() {
    let image = sparse::parse(&mut Cursor::new(sparse_bytes()), 512).unwrap();
    assert_eq!((image.expanded_bytes, image.transfer_bytes), (1536, 1024));
    assert_eq!(image.chunks[0].data, Data::Raw { file_offset: 40 });
    assert_eq!(image.chunks[1].data, Data::DontCare);
    assert_eq!((image.chunks[2].offset, image.chunks[2].bytes), (1024, 512));
    assert_eq!(image.chunks[2].data, Data::Fill([1, 2, 3, 4]));
}
#[test]
fn sparse_rejects_truncation_bad_totals_versions_and_unsupported_chunks() {
    let original = sparse_bytes();
    for end in [0, 27, 39, 100, original.len() - 1] {
        assert!(sparse::parse(&mut Cursor::new(&original[..end]), 512).is_err());
    }
    for (pos, bytes) in [
        (4, 2u32.to_le_bytes()),
        (16, 4u32.to_le_bytes()),
        (28, 0xcac4u32.to_le_bytes()),
        (36, 12u32.to_le_bytes()),
    ] {
        let mut b = original.clone();
        b[pos..pos + 4].copy_from_slice(&bytes);
        assert!(sparse::parse(&mut Cursor::new(b), 512).is_err());
    }
    assert!(sparse::parse(&mut Cursor::new(original), 4096).is_err());
}
#[test]
fn xml_preserves_file_node_order_and_explicit_skips() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("raw.bin"), [7; 1535]).unwrap();
    fs::write(root.join("sparse.bin"), sparse_bytes()).unwrap();
    let a = xml(root, "a.xml", &(program("raw.bin", "") + &program("", "")));
    let b = xml(root, "b.xml", &program("sparse.bin", "sparse=\"true\""));
    let p = xml(root, "p.xml", &(patch("raw.bin") + &patch("DISK")));
    let list = program_xml::parse(&[a.clone(), b], &[p], 512, false).unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0].xml, a);
    assert_eq!(list[1].xml_node, 2);
    assert!(matches!(
        list[0].operation,
        Operation::Program {
            transfer_bytes: 1536,
            ..
        }
    ));
    assert!(matches!(list[1].operation, Operation::Skip { .. }));
    assert!(matches!(
        list[2].operation,
        Operation::Program {
            transfer_bytes: 1024,
            ..
        }
    ));
    assert!(matches!(list[3].operation, Operation::Skip { .. }));
    assert!(matches!(list[4].operation, Operation::Patch { .. }));
}
#[test]
fn xml_preflight_rejects_missing_invalid_ranges_and_unknown_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("raw.bin"), [0; 1024]).unwrap();
    let good = program("raw.bin", "");
    for body in [
        program("missing.bin", ""),
        good.clone(),
        good.replace(
            "start_sector=\"64\"",
            "start_sector=\"18446744073709551615\"",
        ),
        good.replace(
            "num_partition_sectors=\"3\"",
            "num_partition_sectors=\"invalid\"",
        ),
        "<erase/>".into(),
        good.replace("<program", "<read"),
        good.replace("512", "4096"),
    ] {
        let file = xml(root, "bad.xml", &body);
        assert!(
            program_xml::parse(&[file], &[], 512, false).is_err(),
            "{body}"
        );
    }
    let skip = xml(root, "skip.xml", &program("", ""));
    assert!(program_xml::parse(&[skip], &[], 512, false).is_err());
    let local = xml(root, "local.xml", &patch("raw.bin"));
    assert!(program_xml::parse(&[], &[local], 512, false).is_err());
    assert!(program_xml::parse(&[], &[], 512, true).is_err());
}
#[test]
fn raw_offsets_allow_only_final_sector_padding() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("raw.bin"), [0; 2047]).unwrap();
    let file = xml(
        root,
        "offset.xml",
        &program("raw.bin", "file_sector_offset=\"1\""),
    );
    let list = program_xml::parse(&[file], &[], 512, false).unwrap();
    assert!(matches!(
        list[0].operation,
        Operation::Program {
            source_offset: 512,
            transfer_bytes: 1536,
            ..
        }
    ));
}
#[test]
fn protocol_expressions_are_limited_not_evaluated() {
    for s in ["0", "NUM_DISK_SECTORS-1.", "CRC32(NUM_DISK_SECTORS-1.,512)"] {
        assert!(program_xml::expression(s, true).is_ok());
    }
    for s in [
        "",
        "-1",
        "NUM_DISK_SECTORS--1",
        "CRC32(0,x)",
        "1;reset",
        "18446744073709551616",
    ] {
        assert!(program_xml::expression(s, true).is_err());
    }
}

fn chunked_sparse(block: u32, chunks: &[(u16, u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(sparse::MAGIC.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    b.extend(28u16.to_le_bytes());
    b.extend(12u16.to_le_bytes());
    for n in [
        block,
        chunks.iter().map(|c| c.1).sum(),
        chunks.len() as u32,
        0,
    ] {
        b.extend(n.to_le_bytes());
    }
    for (kind, blocks, payload) in chunks {
        b.extend(kind.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(blocks.to_le_bytes());
        b.extend((12 + payload.len() as u32).to_le_bytes());
        b.extend(payload);
    }
    b
}
#[test]
fn smaller_sparse_blocks_produce_aligned_raw_fill_and_gap_candidates() {
    let input = chunked_sparse(
        512,
        &[
            (0xcac1, 8, vec![9; 4096]),
            (0xcac3, 8, vec![]),
            (0xcac2, 8, vec![1, 2, 3, 4]),
        ],
    );
    let image = sparse::parse(&mut Cursor::new(&input), 4096).unwrap();
    assert_eq!((image.expanded_bytes, image.transfer_bytes), (12288, 8192));
    assert_eq!(
        image
            .chunks
            .iter()
            .map(|c| (c.offset, c.bytes))
            .collect::<Vec<_>>(),
        [(0, 4096), (4096, 4096), (8192, 4096)]
    );
    assert_eq!(image.chunks[1].data, Data::DontCare);
    assert_eq!(image.chunks[2].data, Data::Fill([1, 2, 3, 4]));
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("aligned.bin"), input).unwrap();
    let file = xml(
        tmp.path(),
        "aligned.xml",
        &program("aligned.bin", "sparse=\"true\"").replace("512", "4096"),
    );
    let list = program_xml::parse(&[file], &[], 4096, false).unwrap();
    assert!(matches!(
        list[0].operation,
        Operation::Program {
            transfer_bytes: 8192,
            span_bytes: 12288,
            ..
        }
    ));
    for kind in [0xcac1, 0xcac2, 0xcac3] {
        let payload = match kind {
            0xcac1 => vec![0; 512],
            0xcac2 => vec![0; 4],
            _ => vec![],
        };
        assert!(
            sparse::parse(
                &mut Cursor::new(chunked_sparse(512, &[(kind, 1, payload)])),
                4096
            )
            .is_err()
        );
    }
}
#[test]
fn zero_chunks_are_noops_only_after_type_and_payload_validation() {
    let image = sparse::parse(
        &mut Cursor::new(chunked_sparse(
            512,
            &[
                (0xcac1, 0, vec![]),
                (0xcac2, 0, vec![1; 4]),
                (0xcac3, 0, vec![]),
                (0xcac1, 8, vec![7; 4096]),
            ],
        )),
        4096,
    )
    .unwrap();
    assert_eq!(
        (
            image.expanded_bytes,
            image.transfer_bytes,
            image.chunks.len()
        ),
        (4096, 4096, 1)
    );
    assert_eq!(image.chunks[0].offset, 0);
    assert_eq!(image.chunks[0].data, Data::Raw { file_offset: 80 });
    for (kind, payload) in [
        (0xcac1, vec![1]),
        (0xcac2, vec![]),
        (0xcac3, vec![1]),
        (0xcac4, vec![]),
    ] {
        assert!(
            sparse::parse(
                &mut Cursor::new(chunked_sparse(512, &[(kind, 0, payload)])),
                4096
            )
            .is_err()
        );
    }
}
