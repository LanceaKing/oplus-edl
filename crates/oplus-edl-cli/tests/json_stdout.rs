//! A separate process captures real stdout, including dependency logs and progress.
//! No test harness output is mixed into the protocol result.
use oplus_edl_core::result::VerificationState;
#[path = "../../oplus-edl-core/tests/support/package_fixture.rs"]
mod package_fixtures;
#[path = "../../oplus-edl-core/tests/support/mod.rs"]
mod support;
use oplus_edl_cli::{
    OutputMode, OutputWriter,
    args::{Cli, Command, RestoreCommand},
    run_with_service,
};

use clap::Parser;
use oplus_edl_core::{
    CoreService, device,
    model::Verify,
    restore,
    result::{Report, Status},
    storage::Partition,
    write::Writes,
};
use support::*;

fn packet(command: u32, args: &[u32]) -> Vec<u8> {
    [command, 8 + 4 * args.len() as u32]
        .into_iter()
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect()
}

fn child(with_digest: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("image.bin");
    std::fs::write(&path, [7; 511]).unwrap();
    let mappings = restore::single(
        &path,
        "boot",
        Some(0),
        &[Partition {
            lun: 0,
            name: "boot".into(),
            start_sector: 64,
            sectors: 2,
            sector_size: 512,
        }],
    )
    .unwrap();
    let writes = Writes::restore(vec![mappings], 512).unwrap();
    let mut expected = vec![7; 511];
    expected.push(0);
    let mut input = vec![
        bytes(&packet(1, &[2, 1, 4096, 0, 0, 0, 0, 0, 0, 0])),
        bytes(&packet(3, &[13, 0, 4])), bytes(&packet(4, &[13, 0])), bytes(&packet(6, &[1])),
        bytes(b"<data><log value=\"INFO: End of supported functions\"/></data>"),
        bytes(b"<data><response value=\"ACK\" MinVersionSupported=\"1\" Version=\"1\" MaxPayloadSizeToTargetInBytesSupported=\"4096\" MaxPayloadSizeToTargetInBytes=\"4096\" MaxXMLSizeInBytes=\"4096\"/></data>"),
    ];
    if with_digest {
        input.insert(5, bytes(ACK));
    }
    input.extend([
        bytes(LOG),
        bytes(ACK),
        bytes(ACK),
        bytes(LOG),
        bytes(ACK),
        bytes(&expected),
        bytes(LOG),
        bytes(ACK),
    ]);
    let (mut dev, trace) = storage_device(input, 512);
    let cli = Cli::parse_from([
        "oplus-edl",
        "--json",
        "restore",
        "image",
        "--memory",
        "emmc",
        "--image",
        "unused",
        "--partition",
        "0:boot",
    ]);
    let Command::Restore(RestoreCommand::Image { connection, .. }) = cli.command else {
        panic!("expected image restore");
    };
    dev.fh_cfg = device::configuration(
        connection.backend,
        connection.memory,
        connection.sector_size,
    )
    .unwrap();
    if with_digest {
        device::initialize_with_progress(
            &mut dev,
            &mut [Some(b"abcd".to_vec())],
            Some(&[0xa5; 512]),
            |message| eprintln!("{message}"),
        )
        .unwrap();
    } else {
        device::initialize(&mut dev, &mut [Some(b"abcd".to_vec())]).unwrap();
    }
    let mut report = Report::new(Some("restore"), Some("best-effort"));
    writes.execute(&mut dev, Verify::BestEffort, &mut report);
    drop(dev);
    assert_no_reset(&trace);
    assert_eq!(report.status, Status::Success);
    assert_eq!(report.items[0].verification, VerificationState::Matched);
    OutputWriter::new(
        OutputMode {
            json: true,
            quiet: true,
        },
        std::io::stdout(),
    )
    .finish(&report)
    .unwrap();
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--prepared-test-child"))
    {
        let args = std::iter::once("oplus-edl".into()).chain(std::env::args_os().skip(2));
        let (host, trace) = scripted_host(package_project_script(23632));
        let code = run_with_service(
            Cli::parse_from(args),
            CoreService::new(host),
            std::io::stdout(),
            |_| {},
        );
        let commands = wire_commands(&trace);
        assert_eq!(commands.len(), 14);
        assert_eq!(commands[0].name, "configure");
        assert!(commands[1..].iter().all(|command| command.name == "read"));
        assert!(trace.lock().unwrap().closed);
        assert_no_reset(&trace);
        std::process::exit(code);
    }
    if std::env::args().any(|arg| arg == "--protocol-child") {
        child(std::env::args().any(|arg| arg == "--with-digest"));
        return;
    }
    for with_digest in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.arg("--protocol-child");
        if with_digest {
            command.arg("--with-digest");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout)
            .expect("exactly one JSON object, without dependency logs or progress");
        assert_eq!(value["outcome"], "success");
        assert_eq!(value["items"][0]["write"], "acknowledged");
        assert_eq!(value["items"][0]["verification"], "matched");
        if with_digest {
            assert!(
                String::from_utf8_lossy(&result.stderr)
                    .contains("signed digest table acknowledged")
            );
        } else {
            assert!(
                result.stderr.is_empty(),
                "quiet protocol emitted stderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    two_process_package_guard();
    println!("JSON stdout protocol subprocess: passed");
}

fn two_process_package_guard() {
    use serde_json::Value;
    use std::{fs, path::Path, process::Command};

    fn artifacts(path: &Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect()
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_fixtures::package_with_programs(root);
    let output = root.join("prepared");
    let loader = root.join("loader.elf");
    fs::write(&loader, b"abcd").unwrap();
    let prepare = Command::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(["package", "prepare", "--json", "-q", "--package"])
        .arg(root)
        .args(["--nv-id", "123", "--output"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        prepare.status.success(),
        "{}",
        String::from_utf8_lossy(&prepare.stdout)
    );
    assert!(prepare.stderr.is_empty());
    let result: Value = serde_json::from_slice(&prepare.stdout).unwrap();
    assert_eq!(result["data"]["preparation"]["origin"]["nv_id"], "123");
    assert_eq!(result["session_generation"], Value::Null);
    assert_eq!(result["target"], Value::Null);

    let original = artifacts(&output);
    // The second process uses the production parser and CLI adapter with a scripted host.
    let flash = Command::new(std::env::current_exe().unwrap())
        .args(["--prepared-test-child", "flash", "package", "--package"])
        .arg(root)
        .args(["--nv-id", "123", "--prepared"])
        .arg(&output)
        .arg("--loader")
        .arg(&loader)
        .args(["--yes", "--verify", "none", "--reboot", "--json", "-q"])
        .output()
        .unwrap();
    assert_eq!(
        flash.status.code(),
        Some(2),
        "{}\n{}",
        String::from_utf8_lossy(&flash.stdout),
        String::from_utf8_lossy(&flash.stderr)
    );
    assert!(flash.stderr.is_empty());
    let result: Value = serde_json::from_slice(&flash.stdout).unwrap();
    assert_eq!(result["outcome"], "input_error");
    assert!(
        result["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("device OCDT project 23632 does not match package project 23631")
    );
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["operation"] == "read" && item["execution"] == "done")
    );
    assert!(result["outputs"].as_array().unwrap().is_empty());
    assert_eq!(artifacts(&output), original);
}
