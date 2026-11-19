//! Subprocess deadline keeps a regression from hanging the test runner.
#[cfg(unix)]
mod unix {
    use oplus_edl_core::{program_xml, restore, storage::Partition};
    use std::{
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn special_input_child() {
        let Some(path) = std::env::var_os("OPLUS_EDL_TEST_SPECIAL_INPUT") else {
            return;
        };
        let path = PathBuf::from(path);
        let mode = std::env::var("OPLUS_EDL_TEST_INPUT_MODE").unwrap();
        let error = match mode.as_str() {
            "restore" => restore::single(
                &path,
                "boot",
                Some(0),
                &[Partition {
                    lun: 0,
                    name: "boot".into(),
                    start_sector: 64,
                    sectors: 8,
                    sector_size: 512,
                }],
            )
            .unwrap_err(),
            "xml" | "program" => program_xml::parse(&[path], &[], 512, false).unwrap_err(),
            _ => panic!("invalid test mode"),
        };
        assert!(
            format!("{error:#}").contains("not a regular file"),
            "{error:#}"
        );
    }

    #[test]
    fn fifo_inputs_are_rejected_without_a_writer() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::symlink},
        };
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("fifo");
        let c_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: valid NUL-terminated path; mkfifo does not retain the pointer.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let link = tmp.path().join("link");
        symlink(&fifo, &link).unwrap();
        let xml = tmp.path().join("program.xml");
        fs::write(&xml, r#"<data><program filename="fifo" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
        for (mode, path) in [
            ("restore", &fifo),
            ("restore", &link),
            ("program", &xml),
            ("xml", &fifo),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "unix::special_input_child", "--nocapture"])
                .env("OPLUS_EDL_TEST_SPECIAL_INPUT", path)
                .env("OPLUS_EDL_TEST_INPUT_MODE", mode)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("{mode} blocked on FIFO without a writer");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{mode}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn ordinary_inputs_still_work_and_directories_are_rejected() {
    use oplus_edl_core::{restore, storage::Partition};
    let tmp = tempfile::tempdir().unwrap();
    let target = Partition {
        lun: 0,
        name: "boot".into(),
        start_sector: 64,
        sectors: 2,
        sector_size: 512,
    };
    assert!(restore::single(tmp.path(), "boot", Some(0), std::slice::from_ref(&target)).is_err());
    let file = tmp.path().join("image");
    std::fs::write(&file, [7; 513]).unwrap();
    let mapping = restore::single(&file, "boot", Some(0), &[target]).unwrap();
    assert_eq!((mapping.source_bytes, mapping.transfer_bytes), (513, 1024));
}
