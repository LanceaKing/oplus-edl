use oplus_edl_core::sparse;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
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
pub fn fixture(root: &Path) -> std::path::PathBuf {
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

pub fn entry(lun: u8, label: &str, file: &str, sectors: u64, sparse: bool) -> String {
    format!(
        r#"<program SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="{lun}" start_sector="0" num_partition_sectors="{sectors}" file_sector_offset="0" sparse="{sparse}" label="{label}" filename="{file}"/>"#
    )
}

pub fn package_with_programs(root: &Path) {
    fixture(root);
    fs::write(root.join("IMAGES/gpt.bin"), [0; 4096]).unwrap();
    for lun in 0..=5 {
        let mut xml = entry(lun, "BackupGPT", "gpt.bin", 1, false);
        if lun == 0 {
            xml += &entry(lun, "super", "super.img", 512, true);
            xml += &entry(lun, "userdata", "system.img", 0, true);
        }
        xml += &entry(lun, "ocdt", "missing.bin", 1, false);
        xml += &entry(lun, "secdata", "sec.dat", 1, false);
        xml += &entry(lun, "PrimaryGPT", "gpt.bin", 1, false);
        fs::write(
            root.join(format!("IMAGES/rawprogram{lun}.xml")),
            format!("<data>{xml}</data>"),
        )
        .unwrap();
        fs::write(root.join(format!("IMAGES/patch{lun}.xml")), "<patches/>").unwrap();
    }
}
