mod support;
use oplus_edl::{
    backup,
    output::{Report, Status},
    restore,
    storage::{self, Partition},
};
use std::fs;
use support::*;
fn targets(lun: u8, names: &[&str]) -> Vec<Partition> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| Partition {
            lun,
            name: (*n).into(),
            start_sector: 64 + i as u64 * 4,
            sectors: 2,
            sector_size: 512,
        })
        .collect()
}
fn backup_script(names: &[&str], readable: usize) -> Vec<Input> {
    let d = disk(512, names);
    let h = gptman::GPTHeader::read_from(&mut std::io::Cursor::new(&d[512..])).unwrap();
    let mut script = gpt_script(&d, 512);
    script.extend([
        bytes(ACK),
        bytes(&d[(h.last_usable_lba as usize + 1) * 512..]),
        bytes(ACK),
    ]);
    for _ in 0..readable {
        script.extend([bytes(ACK), bytes(&[7; 1024]), bytes(ACK)]);
    }
    script
}
#[test]
fn directory_backup_restore_roundtrip_multiple_luns_and_exclusions() {
    let mut script = backup_script(&["boot", "system"], 2);
    script.extend(backup_script(&["boot"], 1));
    let (mut dev, trace) = storage_device(script, 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::directory(&mut dev, &[0, 1], &[], tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::Success);
    fs::write(tmp.path().join("lun0/absent.bin.partial"), [1]).unwrap();
    let mut parts = targets(0, &["boot", "system"]);
    parts.extend(targets(1, &["boot"]));
    let mappings = restore::directory(tmp.path(), None, &[], &parts).unwrap();
    assert_eq!(mappings.len(), 3);
    assert_eq!(
        mappings.iter().map(|m| m.target.lun).collect::<Vec<_>>(),
        [0, 0, 1]
    );
    assert!(mappings.iter().all(|m| m.transfer_bytes == 1024));
    assert_eq!(
        restore::directory(tmp.path(), Some(1), &["boot".into()], &parts)
            .unwrap()
            .len(),
        1
    );
    assert_no_reset(&trace);
    assert!(!String::from_utf8_lossy(&trace.lock().unwrap().accepted).contains("<program"));
}
#[test]
fn unsafe_and_colliding_names_do_not_prevent_other_backups() {
    let names = ["boot", "Boot", "../bad", "good"];
    let (mut dev, trace) = storage_device(backup_script(&names, 1), 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::directory(&mut dev, &[0], &[], tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::Unsupported);
    assert_eq!(
        report.items.iter().filter(|i| !i.errors.is_empty()).count(),
        3
    );
    assert!(tmp.path().join("lun0/good.bin").exists());
    assert!(!tmp.path().join("lun0/boot.bin").exists());
    assert_no_reset(&trace);
}
#[test]
fn restore_preflights_layout_names_lengths_and_unique_targets() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("boot.bin"), [1; 513]).unwrap();
    let parts = targets(0, &["boot", "system"]);
    assert!(restore::directory(tmp.path(), None, &[], &parts).is_err());
    let m = restore::directory(tmp.path(), Some(0), &[], &parts).unwrap();
    assert_eq!(
        (m[0].source_bytes, m[0].transfer_bytes, m[0].uncovered_bytes),
        (513, 1024, 0)
    );
    assert!(restore::directory(tmp.path(), Some(0), &["system".into()], &parts).is_err());
    assert!(restore::directory(tmp.path(), Some(0), &[], &targets(0, &["boot", "boot"])).is_err());
    assert!(restore::directory(tmp.path(), Some(0), &[], &targets(0, &["boot", "Boot"])).is_err());
    fs::create_dir(tmp.path().join("lun0")).unwrap();
    assert!(restore::directory(tmp.path(), Some(0), &[], &parts).is_err());
    fs::write(tmp.path().join("boot.bin"), [1; 1025]).unwrap();
    assert!(restore::single(&tmp.path().join("boot.bin"), "boot", Some(0), &parts).is_err());
}
#[test]
fn single_image_allows_nonportable_names_but_never_ambiguous_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("image");
    fs::write(&file, [1]).unwrap();
    let mut parts = targets(0, &["../boot"]);
    parts.extend(targets(1, &["../boot"]));
    assert!(restore::single(&file, "../boot", None, &parts).is_err());
    let m = restore::single(&file, "../boot", Some(1), &parts).unwrap();
    assert_eq!((m.transfer_bytes, m.uncovered_bytes), (512, 512));
}
#[test]
fn discovery_requires_explicit_consistent_counts() {
    let (mut dev,trace)=storage_device([bytes(b"<data><log value='INFO: {&quot;storage_info&quot;:{&quot;num_physical&quot;:2,&quot;block_size&quot;:512}}'/></data>"),bytes(ACK)],512);
    let mut info = qdl::firehose_get_storage_info(&mut dev, 0).unwrap();
    assert_eq!(storage::discovered_luns(&info, 512).unwrap(), [0, 1]);
    assert!(storage::discovered_luns(&info, 4096).is_err());
    info.attributes.insert("num_physical".into(), "3".into());
    assert!(storage::discovered_luns(&info, 512).is_err());
    info.attributes.clear();
    info.logs.clear();
    assert!(storage::discovered_luns(&info, 512).is_err());
    assert_no_reset(&trace);
}

#[test]
fn read_failure_keeps_completed_partition_and_stops_remaining_reads() {
    let mut script = backup_script(&["first", "second", "third"], 1);
    script.extend([bytes(ACK), bytes(&[8; 100]), Input::Eof]);
    let (mut dev, trace) = storage_device(script, 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::directory(&mut dev, &[0, 1], &[], tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::ExecutionFailed);
    assert!(tmp.path().join("lun0/first.bin").exists());
    assert_eq!(
        fs::read(tmp.path().join("lun0/second.bin.partial")).unwrap(),
        [8; 100]
    );
    let last = report.items.last().unwrap();
    assert_eq!(last.partition.as_deref(), Some("third"));
    assert_eq!(last.execution, "not_attempted");
    assert!(!tmp.path().join("lun1").exists());
    assert_no_reset(&trace);
    assert_eq!(trace.lock().unwrap().writes.len(), 5);
}
#[test]
fn partial_directory_needs_only_selected_images_and_keeps_lun_meaning() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("lun2")).unwrap();
    fs::write(tmp.path().join("lun2/boot.bin"), [1; 512]).unwrap();
    let parts = targets(2, &["boot", "system"]);
    assert_eq!(
        restore::directory(tmp.path(), None, &[], &parts)
            .unwrap()
            .len(),
        1
    );
    assert!(restore::directory(tmp.path(), Some(0), &[], &parts).is_err());
    assert!(restore::directory(&tmp.path().join("lun2"), None, &[], &parts).is_err());
    assert_eq!(
        restore::directory(&tmp.path().join("lun2"), Some(2), &[], &parts)
            .unwrap()
            .len(),
        1
    );
}
