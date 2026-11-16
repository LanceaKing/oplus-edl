mod support;
use oplus_edl::{
    backup,
    output::{Item, Report, Status},
    storage::*,
};
use qdl::types::MutationConfirmation;
use std::io::{self, Cursor, ErrorKind, Read, Write};
use support::*;

fn part() -> Partition {
    Partition {
        lun: 2,
        name: "boot".into(),
        start_sector: 64,
        sectors: 2,
        sector_size: 512,
    }
}
#[test]
fn gpt_and_partition_read_at_both_sector_sizes() {
    for sector in [512, 4096] {
        let disk = disk(sector, &["boot", "system"]);
        let mut script = gpt_script(&disk, sector);
        script.extend([bytes(ACK), bytes(&vec![0x5a; 2 * sector]), bytes(ACK)]);
        let (mut dev, trace) = storage_device(script, sector);
        let table = read_gpt(&mut dev, 2).unwrap();
        let p = find_partition(&table, 2, "boot").unwrap();
        assert_eq!((p.start_sector, p.sectors, p.lun), (64, 2, 2));
        let mut out = Vec::new();
        read_partition(&mut dev, &p, &mut out).unwrap();
        assert_eq!(out, vec![0x5a; 2 * sector]);
        drop(dev);
        assert_no_reset(&trace);
        let wire = String::from_utf8_lossy(&trace.lock().unwrap().accepted).into_owned();
        assert!(wire.contains("start_sector=\"64\""));
        assert!(!wire.contains("<program"));
    }
}
#[test]
fn safe_directory_names() {
    for name in ["boot", "modem_a", "a-b.c", "COM0", "com10"] {
        assert!(safe_name(name), "{name}");
    }
    for name in [
        "", ".", "..", "../boot", "a/b", "a\\b", "boot.", "NUL", "con.bin", "COM1", "lpt9.x",
        "名字", "a:b",
    ] {
        assert!(!safe_name(name), "{name}");
    }
}
#[test]
fn failed_reads_retain_partial_and_never_publish() {
    for final_response in [Input::Error(ErrorKind::TimedOut), bytes(NAK), Input::Eof] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("boot.bin");
        let (mut dev, trace) =
            storage_device([bytes(ACK), bytes(&vec![42; 1024]), final_response], 512);
        let mut item = Item::read(2, Some("boot".into()), 64 * 512, 1024);
        assert!(backup::save_partition(&mut dev, &part(), &path, false, &mut item).is_err());
        assert!(!path.exists());
        assert_eq!(
            std::fs::read(tmp.path().join("boot.bin.partial")).unwrap(),
            vec![42; 1024]
        );
        assert_eq!(item.transferred_bytes.as_deref(), Some("1024"));
        drop(dev);
        assert_no_reset(&trace);
    }
}
#[test]
fn successful_read_publishes_and_existing_files_are_not_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.bin");
    let (mut dev, _) = storage_device([bytes(ACK), bytes(&vec![42; 1024]), bytes(ACK)], 512);
    let mut item = Item::read(2, Some("boot".into()), 64 * 512, 1024);
    backup::save_partition(&mut dev, &part(), &path, false, &mut item).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![42; 1024]);
    assert!(!tmp.path().join("boot.bin.partial").exists());
    let (mut dev, trace) = storage_device([], 512);
    assert!(backup::save_partition(&mut dev, &part(), &path, false, &mut item).is_err());
    std::fs::write(tmp.path().join("boot.bin.partial"), b"previous").unwrap();
    assert!(backup::save_partition(&mut dev, &part(), &path, true, &mut item).is_err());
    assert!(trace.lock().unwrap().writes.is_empty());
    assert_eq!(
        std::fs::read(tmp.path().join("boot.bin.partial")).unwrap(),
        b"previous"
    );
}
#[test]
fn complete_backup_captures_gpt_separately_and_marks_scope_limited() {
    let disk = disk(512, &["boot"]);
    let h = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
    let mut script = gpt_script(&disk, 512);
    script.extend([
        bytes(ACK),
        bytes(&disk[(h.last_usable_lba as usize + 1) * 512..]),
        bytes(ACK),
        bytes(ACK),
        bytes(&vec![42; 1024]),
        bytes(ACK),
    ]);
    let (mut dev, trace) = storage_device(script, 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::single_partition(&mut dev, 2, "boot", tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::Success);
    assert!(report.coverage.limited);
    assert_eq!(report.items.len(), 4);
    assert_eq!(
        std::fs::read(tmp.path().join("lun2/.gpt/primary.bin")).unwrap(),
        &disk[..h.first_usable_lba as usize * 512]
    );
    assert_eq!(
        std::fs::read(tmp.path().join("lun2/.gpt/backup.bin")).unwrap(),
        &disk[(h.last_usable_lba as usize + 1) * 512..]
    );
    assert_eq!(
        std::fs::read(tmp.path().join("lun2/boot.bin")).unwrap(),
        vec![42; 1024]
    );
    drop(dev);
    assert_no_reset(&trace);
}
#[test]
fn backup_gpt_failure_stops_before_partition_and_preserves_completed_item() {
    let disk = disk(512, &["boot"]);
    let mut script = gpt_script(&disk, 512);
    script.push(bytes(NAK));
    let (mut dev, trace) = storage_device(script, 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::single_partition(&mut dev, 0, "boot", tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::ExecutionFailed);
    assert_eq!(
        report
            .items
            .iter()
            .map(|i| i.execution.as_str())
            .collect::<Vec<_>>(),
        ["done", "done", "failed", "not_attempted"]
    );
    assert!(!tmp.path().join("lun0/boot.bin").exists());
    drop(dev);
    assert_no_reset(&trace);
}
#[test]
fn output_short_writes_continue_but_zero_and_errors_fail() {
    struct Sink {
        fail: Option<ErrorKind>,
        accepted: usize,
    }
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            match self.fail {
                Some(ErrorKind::WriteZero) => Ok(0),
                Some(e) => Err(e.into()),
                None => {
                    let n = b.len().min(3);
                    self.accepted += n;
                    Ok(n)
                }
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    for fail in [None, Some(ErrorKind::WriteZero), Some(ErrorKind::Other)] {
        let (mut dev, trace) = storage_device([bytes(ACK), bytes(&vec![0; 1024]), bytes(ACK)], 512);
        let mut out = Sink { fail, accepted: 0 };
        let result = read_partition(&mut dev, &part(), &mut out);
        assert_eq!(result.is_ok(), fail.is_none());
        if fail.is_none() {
            assert_eq!(out.accepted, 1024);
        }
        drop(dev);
        assert_no_reset(&trace);
    }
}

#[test]
fn read_eof_timeout_and_initial_nak_preserve_partial_counts() {
    for script in [
        vec![bytes(NAK)],
        vec![bytes(ACK), bytes(&[1; 7]), Input::Eof],
        vec![
            bytes(ACK),
            bytes(&[1; 7]),
            Input::Error(ErrorKind::TimedOut),
        ],
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("boot.bin");
        let (mut dev, trace) = storage_device(script, 512);
        let mut item = Item::read(2, None, 32768, 1024);
        assert!(backup::save_partition(&mut dev, &part(), &path, false, &mut item).is_err());
        assert!(!path.exists());
        let content = std::fs::read(tmp.path().join("boot.bin.partial")).unwrap();
        assert_eq!(
            item.transferred_bytes.as_deref(),
            Some(content.len().to_string().as_str())
        );
        drop(dev);
        assert_no_reset(&trace);
    }
}
#[test]
fn usb_short_completion_and_input_error_do_not_report_success() {
    let (mut dev, trace) = storage_device([bytes(ACK)], 512);
    dev.fh_cfg.backend = qdl::types::QdlBackend::Usb;
    dev.rw
        .output
        .extend([Output::Accept(usize::MAX), Output::Accept(1)]);
    let err = program_partition(&mut dev, &part(), &mut &b"ab"[..], 2).unwrap_err();
    assert_eq!(err.confirmation, MutationConfirmation::Unknown);
    assert_eq!(trace.lock().unwrap().writes.len(), 2);
    drop(dev);
    assert_no_reset(&trace);
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(ErrorKind::Other.into())
        }
    }
    let (mut dev, trace) = storage_device([bytes(ACK)], 512);
    let err = program_partition(&mut dev, &part(), &mut Broken, 2).unwrap_err();
    assert_eq!(err.confirmation, MutationConfirmation::Unknown);
    assert_eq!(trace.lock().unwrap().writes.len(), 1);
}
#[test]
fn overwrite_requires_explicit_flag_and_replaces_only_after_success() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.bin");
    std::fs::write(&path, b"old").unwrap();
    let (mut dev, _) = storage_device([bytes(ACK), bytes(&vec![1; 1024]), bytes(ACK)], 512);
    let mut item = Item::read(2, None, 32768, 1024);
    backup::save_partition(&mut dev, &part(), &path, true, &mut item).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), vec![1; 1024]);
    let (mut dev, _) = storage_device([bytes(ACK), bytes(&[2; 7]), Input::Eof], 512);
    assert!(backup::save_partition(&mut dev, &part(), &path, true, &mut item).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), vec![1; 1024]);
}
#[test]
fn backup_missing_or_duplicate_exact_partition_never_reads_payload() {
    for (names, selected, expected) in [
        (vec!["boot"], "absent", Status::InputError),
        (vec!["boot", "boot"], "boot", Status::Unsupported),
    ] {
        let disk = disk(512, &names);
        let (mut dev, trace) = storage_device(gpt_script(&disk, 512), 512);
        let tmp = tempfile::tempdir().unwrap();
        let mut report = Report::new(Some("backup"), None);
        backup::single_partition(&mut dev, 0, selected, tmp.path(), false, &mut report);
        assert_eq!(report.status, expected);
        assert_eq!(trace.lock().unwrap().writes.len(), 2);
        assert!(tmp.path().join("lun0/.gpt/primary.bin").is_file());
        assert!(!tmp.path().join("lun0/boot.bin").exists());
    }
}

#[test]
fn exact_single_selection_ignores_unselected_case_variant() {
    for name in ["boot", "Boot"] {
        let disk = disk(512, &["boot", "Boot"]);
        let h = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
        let mut script = gpt_script(&disk, 512);
        script.extend([
            bytes(ACK),
            bytes(&disk[(h.last_usable_lba as usize + 1) * 512..]),
            bytes(ACK),
            bytes(ACK),
            bytes(&vec![0x5a; 1024]),
            bytes(ACK),
        ]);
        let (mut dev, trace) = storage_device(script, 512);
        let tmp = tempfile::tempdir().unwrap();
        let mut report = Report::new(Some("backup"), None);
        backup::single_partition(&mut dev, 0, name, tmp.path(), false, &mut report);
        assert_eq!(report.status, Status::Success);
        assert_eq!(
            std::fs::read(tmp.path().join(format!("lun0/{name}.bin"))).unwrap(),
            vec![0x5a; 1024]
        );
        let expected = if name == "boot" { "64" } else { "68" };
        let record = trace.lock().unwrap();
        let last = String::from_utf8_lossy(record.writes.last().unwrap());
        assert!(last.contains(&format!("start_sector=\"{expected}\"")));
    }
}

#[test]
fn gpt_failures_record_the_actual_read_and_retain_exact_partial() {
    for stage in [0, 1] {
        // header probe or full primary capture
        for failure in [0, 1, 2, 3] {
            // partial EOF, partial timeout, final timeout, NAK
            let disk = disk(512, &["boot"]);
            let h = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
            let expected = if stage == 0 {
                &disk[512..1024]
            } else {
                &disk[..h.first_usable_lba as usize * 512]
            };
            let captured = match failure {
                0 | 1 => &expected[..17],
                2 => expected,
                _ => &[],
            };
            let mut script = Vec::new();
            if stage == 1 {
                script.extend([bytes(ACK), bytes(&disk[512..1024]), bytes(ACK)]);
            }
            if failure == 3 {
                script.push(bytes(NAK));
            } else {
                script.extend([bytes(ACK), bytes(captured)]);
                script.push(if failure == 0 {
                    Input::Eof
                } else {
                    Input::Error(ErrorKind::TimedOut)
                });
            }
            let (mut dev, trace) = storage_device(script, 512);
            let tmp = tempfile::tempdir().unwrap();
            let mut report = Report::new(Some("backup"), None);
            backup::single_partition(&mut dev, 0, "boot", tmp.path(), false, &mut report);
            assert_eq!(report.status, Status::ExecutionFailed);
            assert!(report.errors.is_empty());
            assert_eq!(report.items.len(), stage + 1);
            let item = report.items.last().unwrap();
            assert_eq!(item.id, stage + 1);
            assert_eq!(item.execution, "failed");
            assert_eq!(
                item.transferred_bytes.as_deref(),
                Some(captured.len().to_string().as_str())
            );
            assert_eq!(
                item.start_byte.as_deref(),
                Some(if stage == 0 { "512" } else { "0" })
            );
            assert_eq!(
                std::fs::read(tmp.path().join("lun0/.gpt/primary.bin.partial")).unwrap(),
                captured
            );
            assert!(!tmp.path().join("lun0/.gpt/primary.bin").exists());
            assert!(!tmp.path().join("lun0/.gpt/backup.bin").exists());
            assert!(!tmp.path().join("lun0/boot.bin").exists());
            drop(dev);
            assert_eq!(trace.lock().unwrap().writes.len(), stage + 1);
            assert_no_reset(&trace);
        }
    }
}

#[test]
fn invalid_gpt_contents_remain_partial_with_item_error() {
    let mut disk = disk(512, &["boot"]);
    disk[1024] ^= 1;
    let (mut dev, _) = storage_device(gpt_script(&disk, 512), 512);
    let tmp = tempfile::tempdir().unwrap();
    let mut report = Report::new(Some("backup"), None);
    backup::single_partition(&mut dev, 0, "boot", tmp.path(), false, &mut report);
    assert_eq!(report.status, Status::ExecutionFailed);
    assert_eq!(report.items.len(), 2);
    assert_eq!(report.items[1].execution, "failed");
    assert!(tmp.path().join("lun0/.gpt/primary.bin.partial").is_file());
    assert!(!tmp.path().join("lun0/.gpt/primary.bin").exists());
}

#[test]
fn publication_never_replaces_a_concurrently_created_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.bin");
    let mut item = Item::read(0, None, 0, 4);
    let result = backup::save_read(&path, false, &mut item, |out| {
        out.write_all(b"data")?;
        std::fs::write(&path, b"racer")?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"racer");
    assert_eq!(
        std::fs::read(tmp.path().join("boot.bin.partial")).unwrap(),
        b"data"
    );
    assert_ne!(item.execution, "done");
}

#[test]
fn publication_rename_error_keeps_completed_partial() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("boot.bin");
    let mut item = Item::read(0, None, 0, 4);
    assert!(
        backup::save_read(&path, true, &mut item, |out| {
            out.write_all(b"data")?;
            std::fs::create_dir(&path)?;
            Ok(())
        })
        .is_err()
    );
    assert!(path.is_dir());
    assert_eq!(
        std::fs::read(tmp.path().join("boot.bin.partial")).unwrap(),
        b"data"
    );
    assert_ne!(item.execution, "done");
}
