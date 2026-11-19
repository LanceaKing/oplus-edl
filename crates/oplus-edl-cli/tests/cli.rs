use clap::{CommandFactory, Parser};
use oplus_edl_cli::args::{Cli, Command, FlashCommand, RestoreCommand};
use oplus_edl_core::device;
use serde_json::Value;
use std::{
    io::{self, Write},
    process::Command as Process,
    sync::{Arc, Mutex},
};

fn invoke(args: &[&str]) -> (i32, Value) {
    let result = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(args)
        .args(["--json", "-q"])
        .output()
        .unwrap();
    assert!(
        result.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value =
        serde_json::from_slice(&result.stdout).expect("one JSON result without library logs");
    (result.status.code().unwrap(), value)
}

fn check_unread_stdout_deadline(args: &[&str]) {
    use std::{
        process::Stdio,
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };
    let (reader, output) = io::pipe().unwrap();
    let mut seed = output.try_clone().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let increments = count.clone();
    let (first, opened) = mpsc::channel();
    let filler = thread::spawn(move || {
        seed.write_all(&[0]).unwrap();
        increments.store(1, Ordering::Release);
        first.send(()).unwrap();
        while seed.write_all(&[0]).is_ok() {
            increments.fetch_add(1, Ordering::Release);
        }
    });
    let start = Instant::now();
    opened.recv_timeout(Duration::from_secs(3)).unwrap();
    // A blocked single-byte write leaves no space even for the short version text.
    loop {
        let written = count.load(Ordering::Acquire);
        thread::sleep(Duration::from_millis(80));
        if count.load(Ordering::Acquire) == written {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
    }
    let start = Instant::now();
    let mut child = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(args)
        .stdout(output)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let code = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status.code();
        }
        if start.elapsed() >= Duration::from_secs(12) {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    drop(reader);
    filler.join().unwrap();
    assert_eq!(code, Some(74), "early CLI output stalled for {args:?}");
    assert!(start.elapsed() >= Duration::from_secs(5));
}

#[test]
fn real_help_exits_74_with_a_full_unread_stdout_pipe() {
    check_unread_stdout_deadline(&["flash", "package", "--help"]);
}

#[test]
fn real_version_exits_74_with_a_full_unread_stdout_pipe() {
    check_unread_stdout_deadline(&["--version"]);
}

#[test]
fn real_parse_error_exits_74_with_a_full_unread_stdout_pipe() {
    check_unread_stdout_deadline(&["partitions", "--loader", "--json", "-q"]);
}

#[test]
fn human_terminal_renders_initialization_history_even_in_quiet() {
    use crate::stop_support::{NAK, initialization, response, scripted_host};
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    let directory = tempfile::tempdir().unwrap();
    let loader = directory.path().join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    for quiet in [false, true] {
        let mut commands = initialization(false);
        *commands.last_mut().unwrap() = response(NAK);
        let (host, _) = scripted_host(commands);
        let mut args = vec![
            "oplus-edl",
            "partitions",
            "--loader",
            loader.to_str().unwrap(),
        ];
        if quiet {
            args.push("-q");
        }
        let writer = Capture::default();
        let code = run_with_service(
            Cli::parse_from(args),
            CoreService::new(host),
            writer.clone(),
            |_| {},
        );
        assert_eq!(code, 4);
        let text = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
        assert_eq!(text.contains("Sahara: uploading programmer"), !quiet);
        assert!(text.contains("Session reason: exchange_failed"));
        let upload = text.find("Initialization complete: SaharaUpload").unwrap();
        let welcome = text
            .find("Initialization complete: FirehoseWelcome")
            .unwrap();
        assert!(upload < welcome);
        assert!(!text.contains("Initialization complete: ConfigureAck"));
    }
}

fn flash_rejection(validity: &str, status: &str, items: &[Value], message: &str) -> Value {
    serde_json::json!({
        "schema_version": 1, "kind": "result", "seq": "1", "operation_id": "1",
        "session_generation": null, "operation": "flash", "outcome": status,
        "target": null, "coverage": {"limited": true, "reason": "storage scope not discovered"},
        "items": items, "errors": [{"category": status, "code": status, "item_id": null, "message": message, "path": null}],
        "warnings": [], "outputs": [], "stop_reason": null, "data": null,
        "input_validity": validity, "session": {"disposition": "none", "reason": null, "completed_initialization_steps": []},
    })
}
#[test]
fn approved_catalog_and_scope_parse() {
    Cli::command().debug_assert();
    let cli = Cli::command();
    for parent in [
        "devices",
        "partitions",
        "backup",
        "restore",
        "flash",
        "package",
        "cloud",
    ] {
        assert!(cli.find_subcommand(parent).is_some());
    }
    for (parent, child) in [
        ("restore", "image"),
        ("restore", "directory"),
        ("flash", "xml"),
        ("flash", "package"),
        ("package", "prepare"),
        ("cloud", "fetch"),
    ] {
        assert!(
            cli.find_subcommand(parent)
                .unwrap()
                .find_subcommand(child)
                .is_some()
        );
    }
    let cli = Cli::try_parse_from([
        "oplus-edl",
        "restore",
        "image",
        "--image",
        "a.bin",
        "--partition",
        "01:boot:extra",
    ])
    .unwrap();
    assert_eq!(cli.command.verify(), Some("best-effort"));
    let Command::Restore(RestoreCommand::Image { partition, .. }) = cli.command else {
        panic!("expected image restore");
    };
    assert_eq!(partition.lun, Some(1));
    assert_eq!(partition.name, "boot:extra");

    for args in [
        vec!["backup", "--output", "out", "--all"],
        vec![
            "backup",
            "--output",
            "out",
            "--partition",
            "0:boot",
            "--partition",
            "1:boot",
        ],
        vec![
            "restore",
            "directory",
            "--from",
            "backups",
            "--all",
            "--lun",
            "1",
        ],
        vec![
            "cloud",
            "fetch",
            "--config",
            "config.json",
            "--output",
            "assets",
            "--create-identity",
        ],
    ] {
        assert!(
            Cli::try_parse_from(std::iter::once("oplus-edl").chain(args.iter().copied())).is_ok(),
            "{args:?}"
        );
    }
}

#[test]
fn removed_lock_workflows_are_unknown_commands() {
    for command in ["unlock", "lock"] {
        let (code, result) = invoke(&[command]);
        assert_eq!(code, 2);
        assert_eq!(result["outcome"], "input_error");
        assert_eq!(result["operation"], Value::Null);
        assert_eq!(result["operation_id"], Value::Null);
        assert_eq!(result["session_generation"], Value::Null);
        assert_eq!(result["target"], Value::Null);
        assert_eq!(result["items"], serde_json::json!([]));
    }
}
#[test]
fn invalid_inputs_and_phase_boundary_do_not_open_hardware() {
    for args in [
        vec!["restore", "directory", "--from", "/does-not-exist", "--all"],
        vec!["flash", "xml", "--program", "/does-not-exist"],
        vec!["backup", "--output", "x", "--sector-size", "0"],
        vec!["backup", "--output", "x", "--lun", "256"],
        vec!["backup", "--output", "x", "--backend", "bad"],
        vec![
            "backup",
            "--output",
            "x",
            "--device",
            "not-opened",
            "--lun",
            "0",
            "--partition",
            "boot",
        ],
        vec![
            "backup",
            "--output",
            "x",
            "--device",
            "not-opened",
            "--lun",
            "0",
            "--partition",
            "0:boot",
            "--loader",
            "/does-not-exist",
        ],
        vec![
            "restore",
            "directory",
            "--from",
            "x",
            "--image",
            "y",
            "--all",
        ],
    ] {
        let (code, value) = invoke(&args);
        assert_eq!(code, 2, "{args:?}");
        assert_eq!(value["outcome"], "input_error");
        assert!(value["target"].is_null());
    }
    for args in [
        vec!["backup", "--output", "x"],
        vec!["backup", "--output", "x", "--all", "--partition", "0:boot"],
        vec![
            "backup",
            "--output",
            "x",
            "--partition",
            "0:boot",
            "--partition",
            "00:boot",
        ],
        vec![
            "backup",
            "--output",
            "x",
            "--lun",
            "1",
            "--partition",
            "0:boot",
        ],
        vec!["restore", "directory", "--from", "x"],
        vec![
            "restore",
            "directory",
            "--from",
            "x",
            "--all",
            "--partition",
            "0:boot",
        ],
        vec![
            "restore",
            "image",
            "--image",
            "x",
            "--partition",
            "0:boot",
            "--lun",
            "0",
        ],
        vec!["flash", "xml", "--program", "x", "--lun", "0"],
        vec!["devices", "--loader", "y"],
        vec![
            "package",
            "prepare",
            "--package",
            "p",
            "--nv-id",
            "123",
            "--output",
            "x",
            "--yes",
        ],
        vec![
            "cloud", "fetch", "--config", "c", "--output", "x", "--device", "d",
        ],
    ] {
        let (code, result) = invoke(&args);
        assert_eq!(code, 2, "{args:?}: {result}");
        assert_eq!(result["target"], Value::Null);
    }
}
#[test]
fn selection_requires_unique_explicit_match_without_opening() {
    for keys in [
        vec![],
        vec!["usb:1:2".into()],
        vec!["usb:1:2".into(), "usb:1:3".into()],
    ] {
        for selected in [None, Some("absent")] {
            assert!(
                qdl::select_and_open(&keys, selected, |_| -> anyhow::Result<()> {
                    panic!("must not open")
                })
                .is_err()
            );
        }
    }
    assert!(
        qdl::select_and_open(
            &["same".into(), "same".into()],
            Some("same"),
            |_| -> anyhow::Result<()> { panic!("ambiguous") }
        )
        .is_err()
    );
    assert_eq!(
        qdl::select_and_open(&["a".into(), "b".into()], Some("b"), Ok).unwrap(),
        1
    );
}
#[test]
fn media_defaults_are_explicit_and_writes_are_not_skipped() {
    for (memory, sector) in [("emmc", 512), ("ufs", 4096)] {
        let cli = Cli::try_parse_from(["oplus-edl", "partitions", "--memory", memory]).unwrap();
        let Command::Partitions { connection, .. } = cli.command else {
            panic!("expected partitions");
        };
        let config = device::configuration(
            connection.backend,
            connection.memory,
            connection.sector_size,
        )
        .unwrap();
        assert_eq!(config.storage_sector_size, sector);
        assert!(!config.bypass_storage);
        assert!(config.quiet);
    }
    assert!(Cli::try_parse_from(["oplus-edl", "partitions", "--sector-size", "1024"]).is_err());
}
#[test]
fn removed_features_are_not_accepted() {
    for option in [
        "--resume",
        "--plan-out",
        "--apply-plan",
        "--approve-plan",
        "--cloud-fetch-only",
        "--prepare-only",
    ] {
        assert!(
            Cli::try_parse_from(["oplus-edl", "flash", "xml", "--program", "p.xml", option])
                .is_err()
        );
    }
}
#[test]
fn phase_four_preflight_json_remains_offline_and_reports_unsupported_features() {
    let (code, value) = invoke(&["flash", "xml"]);
    assert_eq!(code, 2);
    assert_eq!(value["outcome"], "input_error");
    let tmp = tempfile::tempdir().unwrap();
    let patch = tmp.path().join("patch.xml");
    let xml = tmp.path().join("raw.xml");
    std::fs::write(&xml, "<data/>").unwrap();
    std::fs::write(&patch, r#"<data><patch filename="DISK" SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="0" start_sector="0" byte_offset="0" size_in_bytes="4" value="1"/></data>"#).unwrap();
    let (code, value) = invoke(&[
        "flash",
        "xml",
        "--program",
        xml.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "--verify",
        "required",
    ]);
    assert_eq!(code, 3);
    assert_eq!(value["items"][0]["execution"], "not_attempted");
    assert!(value["target"].is_null());
    let (code, value) = invoke(&[
        "restore",
        "image",
        "--image",
        "missing",
        "--partition",
        "0:a",
        "--partition",
        "0:b",
    ]);
    assert_eq!(code, 2);
    assert!(value["target"].is_null());
}

#[test]
fn all_skipped_firmware_retains_each_item_in_final_json() {
    let tmp = tempfile::tempdir().unwrap();
    let raw = tmp.path().join("raw.xml");
    let patch = tmp.path().join("patch.xml");
    std::fs::write(&raw,r#"<data><program filename="" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
    std::fs::write(&patch,r#"<data><patch filename="boot.bin" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" byte_offset="0" size_in_bytes="4" value="1"/></data>"#).unwrap();
    for mode in ["best-effort", "required", "none"] {
        let (code, result) = invoke(&[
            "flash",
            "xml",
            "--memory",
            "emmc",
            "--program",
            raw.to_str().unwrap(),
            "--patch",
            patch.to_str().unwrap(),
            "--verify",
            mode,
        ]);
        let items:Vec<_>=[("program",&raw,"empty filename"),("patch",&patch,"local file patch skipped: boot.bin")].into_iter().enumerate().map(|(n,(op,path,reason))|serde_json::json!({
            "id":(n+1).to_string(),"operation":op,"lun":0,"partition":null,"source":{"display":path.to_str().unwrap()},"xml_node":1,
            "start_byte":"32768","source_bytes":null,"partition_bytes":null,"transfer_bytes":null,"transferred_bytes":null,
            "execution":"skipped","write":"not_applicable","verification":"not_applicable","ranges":[],"reason":reason,"errors":[]
        })).collect();
        assert_eq!(code, 3);
        assert_eq!(
            result,
            flash_rejection(
                "valid",
                "unsupported",
                &items,
                "no executable storage operations"
            )
        );
    }
}

#[test]
fn sparse_unsupported_features_and_corrupt_payloads_have_distinct_exit_categories() {
    let tmp = tempfile::tempdir().unwrap();
    let img = tmp.path().join("image.bin");
    let xml = tmp.path().join("raw.xml");
    std::fs::write(&xml,r#"<data><program filename="image.bin" sparse="true" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
    for (kind, version, magic, code, status) in [
        (0xcac4u16, 1u16, 0xed26ff3au32, 3, "unsupported"),
        (0xcaff, 1, 0xed26ff3a, 3, "unsupported"),
        (0xcac1, 1, 0xed26ff3a, 2, "input_error"),
        (0xcac2, 2, 0xed26ff3a, 3, "unsupported"),
        (0xcac2, 1, 0, 2, "input_error"),
    ] {
        let mut b = vec![];
        b.extend(magic.to_le_bytes());
        for v in [version, 0, 28, 12] {
            b.extend(v.to_le_bytes());
        }
        for v in [512u32, 1, 1, 0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(kind.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(16u32.to_le_bytes());
        b.extend([0; 4]);
        std::fs::write(&img, b).unwrap();
        let (actual, r) = invoke(&[
            "flash",
            "xml",
            "--memory",
            "emmc",
            "--program",
            xml.to_str().unwrap(),
        ]);
        assert_eq!(actual, code);
        assert_eq!(r["outcome"], status);
        assert_eq!(r["errors"][0]["category"], status);
        assert_eq!(r["items"], serde_json::json!([]));
        assert!(r["target"].is_null());
        let message = r["errors"][0]["message"].as_str().unwrap().to_owned();
        assert!(!message.is_empty());
        assert_eq!(r, flash_rejection("unchecked", status, &[], &message));
    }
}

#[test]
fn firmware_length_error_identifies_the_xml_source_and_target_without_hardware() {
    let tmp = tempfile::tempdir().unwrap();
    let xml = tmp.path().join("rawprogram0.xml");
    std::fs::write(tmp.path().join("vbmeta_system.img"), [0; 8192]).unwrap();
    std::fs::write(&xml, r#"<data><program filename="vbmeta_system.img" label="vbmeta_system_a" SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="0" start_sector="64" num_partition_sectors="16"/></data>"#).unwrap();
    let (code, result) = invoke(&["flash", "xml", "--program", xml.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(result["target"].is_null());
    let message = result["errors"][0]["message"].as_str().unwrap();
    for detail in [
        "rawprogram0.xml",
        "program node 1",
        "LUN 0",
        "vbmeta_system_a",
        "vbmeta_system.img",
        "65536",
        "8192",
    ] {
        assert!(message.contains(detail), "missing {detail}: {message}");
    }
}

#[test]
fn invalid_signed_digest_is_rejected_before_loader_and_device_access() {
    let tmp = tempfile::tempdir().unwrap();
    let empty = tmp.path().join("empty.mbn");
    let missing = tmp.path().join("missing.mbn");
    std::fs::write(&empty, []).unwrap();
    for path in [&empty, &missing] {
        let (code, result) = invoke(&[
            "backup",
            "--output",
            "unused",
            "--all",
            "--device",
            "not-opened",
            "--loader",
            "not-opened.elf",
            "--signed-digest",
            path.to_str().unwrap(),
        ]);
        assert_eq!(code, 2);
        assert!(result["target"].is_null());
        assert!(
            result["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("signed digest")
        );
        assert_eq!(result["items"], serde_json::json!([]));
    }
}

#[test]
fn cloud_flash_requires_paired_options_and_excludes_local_inputs() {
    for (extra, expected) in [
        (vec!["--cloud-config", "cloud.json"], false),
        (vec!["--cloud-output", "assets"], false),
        (vec!["--create-identity"], false),
        (
            vec!["--cloud-config", "cloud.json", "--cloud-output", "assets"],
            true,
        ),
        (
            vec![
                "--cloud-config",
                "cloud.json",
                "--cloud-output",
                "assets",
                "--create-identity",
            ],
            true,
        ),
        (
            vec![
                "--cloud-config",
                "cloud.json",
                "--cloud-output",
                "assets",
                "--loader",
                "local.elf",
            ],
            false,
        ),
        (
            vec![
                "--cloud-config",
                "cloud.json",
                "--cloud-output",
                "assets",
                "--signed-digest",
                "local.bin",
            ],
            false,
        ),
    ] {
        let base = ["oplus-edl", "flash", "xml", "--program", "program.xml"];
        assert_eq!(
            Cli::try_parse_from(base.into_iter().chain(extra.iter().copied())).is_ok(),
            expected,
            "{extra:?}"
        );
    }
}

#[test]
fn discovery_wait_default_and_explicit_values() {
    let base = ["oplus-edl", "flash", "xml", "--program", "p.xml"];
    for (extra, expected) in [
        (vec![], None),
        (vec!["--wait-timeout", "0"], Some(0)),
        (vec!["--wait-timeout", "30"], Some(30)),
    ] {
        let cli = Cli::try_parse_from(base.into_iter().chain(extra)).unwrap();
        let Command::Flash(FlashCommand::Xml { common, .. }) = cli.command else {
            panic!("expected XML flash");
        };
        assert_eq!(common.connection.local.wait_timeout, expected);
    }
    assert!(Cli::try_parse_from(base.into_iter().chain(["--wait-timeout", "-1"])).is_err());
}

#[path = "../../oplus-edl-core/tests/support/package_fixture.rs"]
mod package_fixtures;
use package_fixtures::package_with_programs;
#[test]
fn package_cli_requires_variant_and_exactly_one_artifact_mode() {
    for (extra, expected) in [
        (vec!["--prepare-output", "out"], false),
        (vec!["--nv-id", "123"], false),
        (vec!["--nv-id", "123", "--prepare-output", "out"], true),
        (vec!["--nv-id", "123", "--prepared", "out"], true),
        (
            vec![
                "--nv-id",
                "123",
                "--prepared",
                "out",
                "--prepare-output",
                "other",
            ],
            false,
        ),
        (
            vec!["--nv-id", "123", "--prepared", "out", "--program", "x"],
            false,
        ),
        (
            vec![
                "--nv-id",
                "123",
                "--prepared",
                "out",
                "--raw-size",
                "declared",
            ],
            false,
        ),
    ] {
        let base = ["oplus-edl", "flash", "package", "--package", "p"];
        assert_eq!(
            Cli::try_parse_from(base.into_iter().chain(extra.iter().copied())).is_ok(),
            expected,
            "{extra:?}"
        );
    }
}

#[test]
fn package_prepare_is_available_offline_through_the_cli() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_with_programs(root);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(["package", "prepare", "--json", "-q", "--package"])
        .arg(root)
        .args(["--nv-id", "123", "--output"])
        .arg(root.join("cli-output"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(report["target"].is_null());
    assert_eq!(report["outcome"], "success");
    assert_eq!(report["session_generation"], Value::Null);
    assert!(result.stderr.is_empty());
    assert_eq!(report["operation"], "package_prepare");
    assert_eq!(report["session"]["disposition"], "none");
    assert_eq!(report["data"]["preparation"]["origin"]["project"], 23631);
    assert_eq!(report["data"]["preparation"]["origin"]["nv_id"], "123");
    assert_eq!(
        report["data"]["preparation"]["exclusions"][0]["label"],
        "ocdt"
    );
    assert_eq!(report["outputs"].as_array().unwrap().len(), 7);
    assert!(root.join("cli-output/selection.json").is_file());
}

#[path = "../../oplus-edl-core/tests/support/mod.rs"]
mod stop_support;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn terminal(&self) -> Value {
        let bytes = self.0.lock().unwrap();
        assert_eq!(bytes.iter().filter(|&&byte| byte == b'\n').count(), 1);
        serde_json::from_slice(&bytes).unwrap()
    }
}

mod response_wait {
    use super::{Capture, stop_support::*};
    use clap::Parser;
    use oplus_edl_cli::{OutputMode, OutputWriter, args::Cli, run_with_service};
    use oplus_edl_core::{
        events::Event,
        result::{Error, Item, Report, Status},
        session::ResponseWaitExpired,
        test_support::{service_with_lun_query_gate, service_with_stop_clock},
    };
    use serde_json::Value;
    use std::{fs, io::ErrorKind, path::Path};

    fn args(dir: &Path, command: &[&str], params: &[&str]) -> Cli {
        let loader = dir.join("loader.elf");
        fs::write(&loader, b"abcd").unwrap();
        let mut arguments = vec!["oplus-edl", "--json", "-q"];
        arguments.extend(command);
        arguments.extend(["--device", "scripted", "--loader", loader.to_str().unwrap()]);
        arguments.extend(params.iter().copied());
        Cli::parse_from(arguments)
    }

    fn flash_args(dir: &Path, verify: &str, digest: bool) -> Cli {
        let sector = if digest { 4096 } else { 512 };
        let image = dir.join("boot.img");
        fs::write(image, vec![7; sector]).unwrap();
        let xml = dir.join("program.xml");
        fs::write(&xml, format!(
            r#"<data><program label="boot" filename="boot.img" SECTOR_SIZE_IN_BYTES="{sector}" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#
        )).unwrap();
        let mut params = vec![
            "--yes",
            "--verify",
            verify,
            "--program",
            xml.to_str().unwrap(),
            "--reboot",
        ];
        let table = dir.join("table.bin");
        let signature = dir.join("signature.bin");
        if digest {
            fs::write(&table, [0xaa; 512]).unwrap();
            fs::write(&signature, [0xbb; 512]).unwrap();
            params.extend([
                "--signed-digest",
                table.to_str().unwrap(),
                "--digest-signature",
                signature.to_str().unwrap(),
            ]);
        } else {
            params.extend(["--memory", "emmc"]);
        }
        args(dir, &["flash", "xml"], &params)
    }

    fn run_script(
        cli: Cli,
        script: Vec<Input>,
        pause: Pause,
        clock: ManualClock,
        at: u64,
    ) -> (i32, Value, Trace) {
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let writer = Capture::default();
        let code = run_with_service(cli, service, writer.clone(), move |relay| {
            std::thread::spawn(move || {
                pause.arrive();
                relay();
                clock.set(at);
                pause.release();
            });
        });
        (code, writer.terminal(), trace)
    }

    #[test]
    fn partitions_signal_stops_one_run_between_initialization_and_the_first_query() {
        let temp = tempfile::tempdir().unwrap();
        let cli = args(temp.path(), &["partitions"], &["--memory", "emmc"]);
        let (host, trace) = scripted_host(initialization(false));
        let (pause, callback) = Pause::new();
        let service =
            service_with_lun_query_gate(host, ManualClock::default().clock(), Box::new(callback));
        let writer = Capture::default();
        let code = run_with_service(cli, service, writer.clone(), move |relay| {
            pause.arrive();
            relay();
            pause.release();
        });
        assert_eq!(code, 130);
        let result = writer.terminal();
        assert_eq!(result["kind"], "result");
        assert_eq!(result["outcome"], "stopped");
        assert_eq!(result["stop_reason"], "signal");
        assert_eq!(result["data"], Value::Null);
        assert!(result["operation_id"].is_string());
        assert!(result["session_generation"].is_string());
        assert_eq!(result["session"]["disposition"], "closed");
        assert_eq!(
            result["session"]["completed_initialization_steps"],
            serde_json::json!(["sahara_upload", "firehose_welcome", "configure_ack"])
        );
        assert!(trace.lock().unwrap().closed);
        assert_eq!(
            wire_commands(&trace)
                .iter()
                .map(|cmd| cmd.name.as_str())
                .collect::<Vec<_>>(),
            ["configure"]
        );
    }

    #[test]
    fn cli_program_stop_serializes_unknown_or_retained_ack_with_mode_specific_exit_codes() {
        for (verify, at, expected, code) in [
            ("none", 11, "unknown_write", 6),
            ("best-effort", 4, "verification_incomplete", 5),
            ("required", 4, "verification_incomplete", 5),
            ("none", 4, "stopped", 130),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let cli = flash_args(temp.path(), verify, false);
            let mut script = initialization(false);
            let (ack, pause) = pause_response(bytes(ACK));
            script.extend([response(RAW_ACK), ack]);
            let (exit, result, trace) = run_script(cli, script, pause, ManualClock::default(), at);
            assert_eq!(exit, code, "{result}");
            assert_eq!(result["outcome"], expected);
            let item = &result["items"][0];
            assert_eq!(item["id"], "1");
            assert_eq!(item["transferred_bytes"], "512");
            if at > 10 {
                assert_eq!(item["write"], "unknown");
                assert_eq!(item["errors"][0]["item_id"], "1");
                assert_eq!(item["errors"][0]["code"], "response_wait_expired");
            } else {
                assert_eq!(item["write"], "acknowledged");
                assert_eq!(
                    item["verification"],
                    if verify == "none" {
                        "not_requested"
                    } else {
                        "incomplete"
                    }
                );
            }
            assert_eq!(result["stop_reason"], "signal");
            assert_eq!(result["session"]["disposition"], "closed");
            assert_eq!(
                wire_commands(&trace)
                    .iter()
                    .map(|cmd| cmd.name.as_str())
                    .collect::<Vec<_>>(),
                ["configure", "program"]
            );
        }
    }

    #[test]
    fn cli_verification_expiry_retains_difference_and_exit_five() {
        for payload in [None, Some(7), Some(6)] {
            let temp = tempfile::tempdir().unwrap();
            let cli = flash_args(temp.path(), "required", false);
            let mut script = initialization(false);
            let (ack, pause) = pause_response(bytes(ACK));
            script.extend([response(RAW_ACK), response(ACK)]);
            if let Some(byte) = payload {
                script.extend([response(RAW_ACK), bytes(&[byte; 512])]);
            }
            script.push(ack);
            let (code, result, trace) = run_script(cli, script, pause, ManualClock::default(), 11);
            assert_eq!(code, 5, "{result}");
            assert_eq!(result["outcome"], "verification_incomplete");
            let item = &result["items"][0];
            assert_eq!(item["write"], "acknowledged");
            assert_eq!(item["transferred_bytes"], "512");
            assert_eq!(item["errors"][0]["code"], "response_wait_expired");
            assert_eq!(item["errors"][0]["item_id"], "1");
            assert_eq!(item["ranges"][0]["verification"], "incomplete");
            assert_eq!(
                item["ranges"][0]["reason"] == "byte_difference_observed",
                payload == Some(6)
            );
            assert_eq!(result["session"]["disposition"], "closed");
            assert_eq!(
                wire_commands(&trace)
                    .iter()
                    .map(|cmd| cmd.name.as_str())
                    .collect::<Vec<_>>(),
                ["configure", "program", "read"]
            );
        }
    }

    #[test]
    fn cli_preflight_and_initialization_expiry_preserve_error_location() {
        let temp = tempfile::tempdir().unwrap();
        let (ack, pause) = pause_response(bytes(STORAGE_ACK));
        let mut script = initialization(false);
        script.push(ack);
        let cli = args(temp.path(), &["partitions"], &["--memory", "emmc"]);
        let (code, result, _) = run_script(cli, script, pause, ManualClock::default(), 11);
        assert_eq!(code, 4);
        assert!(result["items"].as_array().unwrap().is_empty());
        assert_eq!(result["errors"][0]["code"], "response_wait_expired");
        assert_eq!(result["errors"][0]["item_id"], Value::Null);
        assert_eq!(result["data"], Value::Null);

        for terminal in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let cli = flash_args(temp.path(), "none", true);
            let mut script = initialization(true);
            let (ack, pause) = pause_response(bytes(ACK));
            if terminal {
                script.extend([response(RAW_ACK), bytes(&[0; 5 * 4096])]);
            }
            script.push(ack);
            let (code, result, _) = run_script(cli, script, pause, ManualClock::default(), 11);
            assert_eq!(code, 4, "{result}");
            assert!(result["errors"].as_array().unwrap().is_empty());
            let item = &result["items"][0];
            assert_eq!(item["transfer_bytes"], "20480");
            assert_eq!(
                item["transferred_bytes"],
                if terminal { "20480" } else { "0" }
            );
            assert_eq!(item["errors"][0]["code"], "response_wait_expired");
            assert_eq!(item["errors"][0]["item_id"], item["id"]);
        }

        let temp = tempfile::tempdir().unwrap();
        let cli = flash_args(temp.path(), "none", true);
        let mut script = sahara_script();
        let (idle, pause) = pause_response(Input::Error(ErrorKind::TimedOut));
        script.push(idle);
        let (code, result, _) = run_script(cli, script, pause, ManualClock::default(), 11);
        assert_eq!(code, 4);
        assert_eq!(result["errors"][0]["code"], "response_wait_expired");
        assert_eq!(result["errors"][0]["item_id"], Value::Null);
        assert_eq!(
            result["session"]["completed_initialization_steps"],
            serde_json::json!(["sahara_upload"])
        );
    }

    #[test]
    fn shared_writer_filters_events_and_serializes_the_original_cause() {
        for (json, quiet) in [(false, false), (false, true), (true, false), (true, true)] {
            let writer = Capture::default();
            let mut output = OutputWriter::new(OutputMode { json, quiet }, writer.clone());
            let mut result = Report::new(Some("partitions"), None);
            let mut item = Item::read(0, None, 0, 512);
            item.fail(Status::ExecutionFailed, "test");
            item.errors[0] = Error {
                cause: Some(ResponseWaitExpired),
                ..Error::new(Status::ExecutionFailed, "expiry")
            };
            result.items.push(item);
            result.summarize();
            output
                .write_event(&Event::info("inspection", "read\nprogress"))
                .unwrap();
            assert_eq!(output.finish(&result).unwrap(), 4);
            let bytes = writer.0.lock().unwrap();
            let text = std::str::from_utf8(&bytes).unwrap();
            if json {
                let records: Vec<Value> = text
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(records.len(), if quiet { 1 } else { 2 });
                let terminal = records.last().unwrap();
                assert_eq!(terminal["kind"], "result");
                assert_eq!(terminal["items"][0]["errors"][0]["item_id"], "1");
                assert_eq!(
                    terminal["items"][0]["errors"][0]["code"],
                    "response_wait_expired"
                );
            } else {
                assert_eq!(text.contains("read\nprogress"), !quiet);
                assert!(text.contains("expiry"));
            }
        }
    }
}

#[test]
fn discovery_routes_one_backend_without_opening_a_device() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::{CoreService, HostServices, model::Backend};

    for (option, backend) in [
        (None, Backend::Usb),
        (Some("usb"), Backend::Usb),
        (Some("serial"), Backend::Serial),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut host = HostServices::default();
        for candidate in [Backend::Usb, Backend::Serial] {
            let seen = seen.clone();
            host = host.with_backend(
                candidate,
                move || {
                    seen.lock().unwrap().push(candidate);
                    Ok(vec!["first:device".into(), "second:device".into()])
                },
                |_, _| panic!("discovery opened a device"),
            );
        }
        let mut args = vec!["oplus-edl", "devices", "--json", "--quiet"];
        if let Some(value) = option {
            args.extend(["--backend", value]);
        }
        let cli = Cli::parse_from(args);
        let writer = Capture::default();
        assert_eq!(
            run_with_service(cli, CoreService::new(host), writer.clone(), |_| {}),
            0
        );
        let result = writer.terminal();
        assert_eq!(*seen.lock().unwrap(), [backend]);
        assert_eq!(result["session_generation"], Value::Null);
        assert_eq!(result["session"]["disposition"], "none");
        assert_eq!(
            result["data"]["candidates"],
            serde_json::json!([
                {"key": "first:device", "backend": backend.as_str()},
                {"key": "second:device", "backend": backend.as_str()},
            ])
        );
    }

    let host = HostServices::default().with_backend(
        Backend::Usb,
        || panic!("serial discovery fell back to USB"),
        |_, _| panic!("unexpected open"),
    );
    let cli = Cli::parse_from([
        "oplus-edl",
        "devices",
        "--json",
        "-q",
        "--backend",
        "serial",
    ]);
    let writer = Capture::default();
    assert_eq!(
        run_with_service(cli, CoreService::new(host), writer.clone(), |_| {}),
        3
    );
    assert_eq!(writer.terminal()["outcome"], "unsupported");
}

#[test]
fn real_discovery_attempts_once_without_a_wait_and_rejects_ambiguity() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::{CoreService, HostServices, model::Backend};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let tmp = tempfile::tempdir().unwrap();
    let loader = tmp.path().join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    for (keys, expected_code) in [
        (Vec::new(), "input_error"),
        (vec!["one".to_owned(), "two".to_owned()], "device_ambiguous"),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let host = HostServices::default().with_backend(
            Backend::Usb,
            {
                let calls = calls.clone();
                move || {
                    assert_eq!(calls.fetch_add(1, Ordering::Relaxed), 0);
                    Ok(keys.clone())
                }
            },
            |_, _| panic!("no candidate is selected"),
        );
        let cli = Cli::parse_from([
            "oplus-edl",
            "partitions",
            "--loader",
            loader.to_str().unwrap(),
            "--json",
            "-q",
        ]);
        let writer = Capture::default();
        let code = run_with_service(cli, CoreService::new(host), writer.clone(), |_| {});
        assert_eq!(code, 2);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(writer.terminal()["errors"][0]["code"], expected_code);
    }
}

#[test]
fn directory_restore_keeps_explicit_targets_and_multi_lun_scope() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use std::fs;
    use stop_support::*;

    for explicit in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("backup");
        let loader = tmp.path().join("loader.elf");
        fs::write(&loader, b"abcd").unwrap();
        for lun in 0..=2 {
            let folder = from.join(format!("lun{lun}"));
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("boot.bin"), [lun + 7; 512]).unwrap();
        }
        let mut args = vec![
            "oplus-edl",
            "restore",
            "directory",
            "--from",
            from.to_str().unwrap(),
            "--loader",
            loader.to_str().unwrap(),
            "--memory",
            "emmc",
            "--yes",
            "--verify",
            "none",
            "--json",
            "-q",
        ];
        let wanted: &[u8] = if explicit {
            // LUN 1 also contains a boot image, but only the two explicit targets are selected.
            args.extend([
                "--partition",
                "0:boot",
                "--partition",
                "2:boot",
                "--lun",
                "0",
                "--lun",
                "1",
                "--lun",
                "2",
            ]);
            &[0, 2]
        } else {
            args.extend(["--all", "--lun", "0", "--lun", "1"]);
            &[0, 1]
        };
        let mut script = initialization(false);
        let gpt = disk(512, &["boot", "no_image"]);
        for _ in wanted {
            script.extend(gpt_script(&gpt, 512));
        }
        for _ in wanted {
            script.extend([response(RAW_ACK), response(ACK)]);
        }
        let (host, trace) = scripted_host(script);
        let writer = Capture::default();
        assert_eq!(
            run_with_service(
                Cli::parse_from(args),
                CoreService::new(host),
                writer.clone(),
                |_| {}
            ),
            0
        );
        let terminal = writer.terminal();
        let programs: Vec<_> = terminal["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["operation"] == "program")
            .collect();
        assert_eq!(programs.len(), wanted.len());
        for (item, &lun) in programs.iter().zip(wanted) {
            assert_eq!(item["lun"], lun);
            assert_eq!(item["partition"], "boot");
            assert_eq!(item["write"], "acknowledged");
            assert_eq!(item["transferred_bytes"], "512");
        }
        let commands = wire_commands(&trace);
        assert_eq!(commands.iter().filter(|cmd| cmd.name == "read").count(), 4);
        assert_eq!(
            commands.iter().filter(|cmd| cmd.name == "program").count(),
            2
        );
        assert_no_reset(&trace);
    }
}

#[test]
fn prepared_evidence_does_not_supply_cli_consent() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::{CoreService, HostServices, model::Backend};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    package_with_programs(root);
    let out = root.join("out");
    let prepared = oplus_edl_core::package::prepare(root, "123", &out, None, 4096).unwrap();
    let mut evidence: Value =
        serde_json::from_slice(&std::fs::read(&prepared.manifest).unwrap()).unwrap();
    drop(prepared);
    evidence["device_validated"] = Value::Bool(true);
    evidence["approved"] = Value::Bool(true);
    std::fs::write(
        out.join("selection.json"),
        serde_json::to_vec(&evidence).unwrap(),
    )
    .unwrap();
    let cli = Cli::parse_from([
        "oplus-edl",
        "flash",
        "package",
        "--package",
        root.to_str().unwrap(),
        "--nv-id",
        "123",
        "--prepared",
        out.to_str().unwrap(),
        "--loader",
        "must-not-be-loaded.elf",
        "--json",
        "-q",
    ]);
    let host = HostServices::default().with_backend(
        Backend::Usb,
        || panic!("reopened package reused consent"),
        |_, _| panic!("reopened package opened a device without consent"),
    );
    let writer = Capture::default();
    assert_eq!(
        run_with_service(cli, CoreService::new(host), writer.clone(), |_| {}),
        2
    );
    let terminal = writer.terminal();
    assert_eq!(terminal["outcome"], "input_error");
    assert_eq!(terminal["target"], Value::Null);
    assert_eq!(terminal["session_generation"], Value::Null);
    assert!(
        terminal["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--yes is required")
    );
    assert!(
        terminal["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["write"] == "not_attempted")
    );
}

#[test]
fn flash_xml_requires_yes_and_uses_the_shared_executor_once() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use stop_support::*;
    let temp = tempfile::tempdir().unwrap();
    let loader = temp.path().join("loader.elf");
    let xml = temp.path().join("program.xml");
    std::fs::write(&loader, b"abcd").unwrap();
    std::fs::write(temp.path().join("boot.bin"), [7; 512]).unwrap();
    std::fs::write(&xml, br#"<data><program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();

    for yes in [false, true] {
        let mut args = vec![
            "oplus-edl",
            "flash",
            "xml",
            "--program",
            xml.to_str().unwrap(),
            "--loader",
            loader.to_str().unwrap(),
            "--device",
            "scripted",
            "--memory",
            "emmc",
            "--verify",
            "none",
            "--reboot",
            "--json",
            "-q",
        ];
        if yes {
            args.push("--yes");
        }
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), response(ACK), response(ACK)]);
        let (host, trace) = scripted_host(script);
        let writer = Capture::default();
        let code = run_with_service(
            Cli::parse_from(args),
            CoreService::new(host),
            writer.clone(),
            |_| {},
        );
        let result = writer.terminal();
        let wire = wire_commands(&trace);
        if yes {
            assert_eq!(code, 0);
            assert_eq!(result["items"][0]["write"], "acknowledged");
            assert_eq!(result["items"][0]["start_byte"], "32768");
            assert_eq!(
                wire.iter().map(|cmd| cmd.name.as_str()).collect::<Vec<_>>(),
                ["configure", "program", "power"]
            );
        } else {
            assert_eq!(code, 2);
            assert!(
                result["errors"][0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("--yes is required")
            );
            assert!(wire.is_empty());
        }
    }
}

#[test]
fn cli_receives_only_redacted_device_startup_events() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use stop_support::*;
    let temp = tempfile::tempdir().unwrap();
    let loader = temp.path().join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    let mut script = sahara_script();
    let (configure, pause) = pause_response(bytes(CONFIG_ACK));
    script.extend([
        response(b"<data><log value=\"private fixture token\"/></data>"),
        response(WELCOME_END),
        configure,
    ]);
    script.extend(gpt_script(&disk(512, &["boot"]), 512));
    let (host, _) = scripted_host(script);
    let writer = Capture::default();
    let received = writer.clone();
    let release = std::thread::spawn(move || {
        pause.arrive();
        wait_for(|| {
            String::from_utf8_lossy(&received.0.lock().unwrap())
                .contains("Firehose: startup log received")
                .then_some(())
        });
        pause.release();
    });
    let args = [
        "oplus-edl",
        "partitions",
        "--loader",
        loader.to_str().unwrap(),
        "--device",
        "scripted",
        "--memory",
        "emmc",
        "--lun",
        "0",
        "--json",
    ];
    assert_eq!(
        run_with_service(
            Cli::parse_from(args),
            CoreService::new(host),
            writer.clone(),
            |_| {}
        ),
        0
    );
    release.join().unwrap();
    let bytes = writer.0.lock().unwrap();
    let output = std::str::from_utf8(&bytes).unwrap();
    assert!(!output.contains("private fixture token"));
    assert!(output.lines().any(|line| {
        let event: Value = serde_json::from_str(line).unwrap();
        event["phase"] == "initialization" && event["message"] == "Firehose: startup log received"
    }));
}

#[cfg(unix)]
#[test]
fn restore_image_uses_non_unicode_path_and_serializes_the_actual_source() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use std::{
        ffi::OsString,
        os::unix::ffi::{OsStrExt, OsStringExt},
    };
    use stop_support::*;

    let temp = tempfile::tempdir().unwrap();
    let image = temp
        .path()
        .join(OsString::from_vec(b"boot-\xff.bin".to_vec()));
    let loader = temp.path().join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    let mut args: Vec<OsString> = [
        "oplus-edl",
        "restore",
        "image",
        "--partition",
        "0:boot",
        "--memory",
        "emmc",
        "--json",
        "-q",
        "--yes",
        "--verify",
        "none",
        "--loader",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.extend([
        loader.into_os_string(),
        "--image".into(),
        image.clone().into_os_string(),
    ]);
    let cli = Cli::parse_from(args);
    let Command::Restore(RestoreCommand::Image { image: parsed, .. }) = &cli.command else {
        panic!("expected restore image");
    };
    assert_eq!(parsed, &image);
    let encoded = STANDARD.encode(image.as_os_str().as_bytes());
    if let Err(error) = std::fs::write(&image, [33; 512]) {
        // Some native filesystems reject a non-UTF-8 filename. Test its wire representation directly there.
        assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
        use oplus_edl_cli::{OutputMode, OutputWriter};
        use oplus_edl_core::result::{Item, Report};
        let mut report = Report::new(Some("restore"), None);
        let mut item = Item::read(0, Some("boot".into()), 0, 512);
        item.source = Some(image);
        report.items.push(item);
        report.summarize();
        let writer = Capture::default();
        OutputWriter::new(
            OutputMode {
                json: true,
                quiet: true,
            },
            writer.clone(),
        )
        .finish(&report)
        .unwrap();
        assert_eq!(
            writer.terminal()["items"][0]["source"]["unix_bytes_base64"],
            encoded
        );
        return;
    }
    let mut script = initialization(false);
    script.extend(gpt_script(&disk(512, &["boot"]), 512));
    script.extend([response(RAW_ACK), response(ACK)]);
    let (host, trace) = scripted_host(script);
    let writer = Capture::default();
    assert_eq!(
        run_with_service(cli, CoreService::new(host), writer.clone(), |_| {}),
        0
    );
    let report = writer.terminal();
    let program = &report["items"][2];
    assert_eq!(program["write"], "acknowledged");
    assert_eq!(program["source"]["unix_bytes_base64"], encoded);
    assert!(trace.lock().unwrap().accepted.ends_with(&[33; 512]));
}

#[test]
fn help_version_and_early_errors_keep_stdout_framing() {
    for (args, expected_topic) in [
        (vec!["flash", "package", "--help", "--json"], "help"),
        (vec!["package", "prepare", "--json", "--version"], "version"),
    ] {
        let output = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["data"]["topic"], expected_topic);
        assert_eq!(value["operation_id"], Value::Null);
        assert!(output.stderr.is_empty());
    }
    let (code, version) = invoke(&["--version"]);
    assert_eq!(code, 0);
    assert_eq!(
        version["data"]["text"].as_str().unwrap().trim(),
        concat!("oplus-edl ", env!("CARGO_PKG_VERSION"))
    );
    for args in [
        ["backup", "--output", "--json", "--bogus"].as_slice(),
        ["partitions", "--loader", "--json", "-q"].as_slice(),
        ["partitions", "--loader", "-q", "--json"].as_slice(),
    ] {
        let output = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let value: Value =
            serde_json::from_slice(&output.stdout).expect("one terminal JSON result");
        assert_eq!(value["outcome"], "input_error");
        assert_eq!(value["operation_id"], Value::Null);
    }
    for args in [
        ["backup", "--output=--json", "--all", "--bogus"].as_slice(),
        ["backup", "--all", "--", "--json"].as_slice(),
    ] {
        let output = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        assert!(
            serde_json::from_slice::<Value>(&output.stdout).is_err(),
            "{args:?}"
        );
    }
    let (code, value) = invoke(&["flash", "package", "--typo"]);
    assert_eq!(code, 2);
    assert_eq!(value["kind"], "result");
    assert_eq!(value["operation"], "flash");
    assert_eq!(value["operation_id"], Value::Null);
}

#[test]
fn xml_and_package_help_keep_distinct_raw_size_policies() {
    for (command, values, default) in [
        (
            "xml",
            "[possible values: declared, file]",
            "[default: declared]",
        ),
        ("package", "[possible values: file]", "[default: file]"),
    ] {
        let (code, help) = invoke(&["flash", command, "--help"]);
        assert_eq!(code, 0);
        let help = help["data"]["text"].as_str().unwrap();
        assert!(
            help.contains("--raw-size") && help.contains(default) && help.contains(values),
            "{help}"
        );
    }
    let (code, value) = invoke(&[
        "flash",
        "package",
        "--package",
        "absent",
        "--prepare-output",
        "unused",
        "--nv-id",
        "001",
        "--loader",
        "not-opened",
        "--raw-size",
        "declared",
    ]);
    assert_eq!(code, 2);
    assert_eq!(value["operation_id"], Value::Null);
    assert!(
        value["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("invalid value")
    );
}

#[test]
fn backup_keeps_completed_files_and_rejections_in_all_output_modes() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use std::{fs, io::Cursor};
    use stop_support::*;

    for (json, quiet) in [(false, false), (false, true), (true, false), (true, true)] {
        for stop in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let loader = temp.path().join("loader.elf");
            fs::write(&loader, b"abcd").unwrap();
            let output = temp.path().join("backup");
            let lun_dir = output.join("lun0");
            fs::create_dir_all(&lun_dir).unwrap();
            fs::write(lun_dir.join("existing.bin"), b"existing output").unwrap();
            fs::write(lun_dir.join("blocked.bin.partial"), b"existing partial").unwrap();

            let disk = disk(512, &["existing", "good", "blocked", "later"]);
            let header = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
            let mut script = initialization(false);
            script.extend(gpt_script(&disk, 512));
            script.extend([
                response(RAW_ACK),
                bytes(&disk[(header.last_usable_lba as usize + 1) * 512..]),
                response(ACK),
            ]);
            script.extend([response(RAW_ACK), bytes(&[17; 1024])]);
            let pause = if stop {
                let (ack, pause) = pause_response(bytes(ACK));
                script.push(ack);
                Some(pause)
            } else {
                script.extend([
                    response(ACK),
                    response(RAW_ACK),
                    bytes(&[27; 1024]),
                    response(ACK),
                ]);
                None
            };
            let (host, trace) = scripted_host(script);
            let mut args = vec![
                "oplus-edl",
                "backup",
                "--all",
                "--lun",
                "0",
                "--output",
                output.to_str().unwrap(),
                "--memory",
                "emmc",
                "--loader",
                loader.to_str().unwrap(),
            ];
            if json {
                args.push("--json");
            }
            if quiet {
                args.push("-q");
            }
            let writer = Capture::default();
            let code = run_with_service(
                Cli::parse_from(args),
                CoreService::new(host),
                writer.clone(),
                move |relay| {
                    if let Some(pause) = pause {
                        std::thread::spawn(move || {
                            pause.arrive();
                            relay();
                            pause.release();
                        });
                    }
                },
            );
            assert_eq!(code, 2, "json={json} quiet={quiet} stop={stop}");
            assert_eq!(
                fs::read(lun_dir.join("existing.bin")).unwrap(),
                b"existing output"
            );
            assert_eq!(
                fs::read(lun_dir.join("blocked.bin.partial")).unwrap(),
                b"existing partial"
            );
            assert_eq!(fs::read(lun_dir.join("good.bin")).unwrap(), [17; 1024]);
            if stop {
                assert!(!lun_dir.join("later.bin").exists());
            } else {
                assert_eq!(fs::read(lun_dir.join("later.bin")).unwrap(), [27; 1024]);
            }
            assert_no_reset(&trace);
            let bytes = writer.0.lock().unwrap();
            let text = std::str::from_utf8(&bytes).unwrap();
            if json {
                let records: Vec<Value> = text
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert!(!quiet || records.len() == 1);
                assert!(
                    records[..records.len() - 1]
                        .iter()
                        .all(|record| record["kind"] == "event")
                );
                let result = records.last().unwrap();
                assert_eq!(result["kind"], "result");
                assert_eq!(result["outcome"], "input_error");
                assert_eq!(
                    result["stop_reason"],
                    if stop {
                        Value::String("signal".into())
                    } else {
                        Value::Null
                    }
                );
                let items = result["items"].as_array().unwrap();
                let good = items
                    .iter()
                    .find(|item| item["partition"] == "good")
                    .unwrap();
                assert_eq!(good["execution"], "done");
                assert_eq!(good["transferred_bytes"], "1024");
                let rejected: Vec<_> = items
                    .iter()
                    .filter(|item| item["errors"][0]["code"] == "input_error")
                    .collect();
                assert_eq!(rejected.len(), if stop { 1 } else { 2 });
                assert!(
                    result["outputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|retained| {
                            retained["path"]["display"]
                                .as_str()
                                .unwrap()
                                .ends_with("good.bin")
                                && retained["state"] == "complete"
                        })
                );
            } else {
                assert!(text.contains("backup: InputError"));
                assert!(
                    text.lines()
                        .any(|line| line.starts_with("Output complete (partition)")
                            && line.contains("good.bin"))
                );
                assert_eq!(
                    text.matches("output or partial already exists").count(),
                    if stop { 1 } else { 2 }
                );
                assert_eq!(text.contains("Stop: Signal"), stop);
            }
        }
    }
}

#[test]
fn backup_progress_and_real_owned_partial_follow_all_four_output_modes() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::CoreService;
    use std::{fs, io::Cursor, thread};
    use stop_support::*;

    for json in [false, true] {
        for quiet in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let loader = root.path().join("loader.bin");
            fs::write(&loader, b"abcd").unwrap();
            let output = root.path().join("backup");
            let args = Cli::parse_from(
                [
                    "oplus-edl",
                    "backup",
                    "--partition",
                    "0:boot",
                    "--output",
                    output.to_str().unwrap(),
                    "--memory",
                    "emmc",
                    "--loader",
                    loader.to_str().unwrap(),
                ]
                .into_iter()
                .chain(json.then_some("--json"))
                .chain(quiet.then_some("-q")),
            );
            let image = disk(512, &["boot"]);
            let header = gptman::GPTHeader::read_from(&mut Cursor::new(&image[512..])).unwrap();
            let mut script = initialization(false);
            script.extend(gpt_script(&image, 512));
            script.extend([
                response(RAW_ACK),
                bytes(&image[(header.last_usable_lba as usize + 1) * 512..]),
                response(ACK),
            ]);
            let (missing_ack, wait) = pause_response(Input::Eof);
            script.extend([response(RAW_ACK), bytes(&[7; 1024]), missing_ack]);
            let (host, _) = scripted_host(script);
            let writer = Capture::default();
            let run = thread::spawn({
                let writer = writer.clone();
                move || run_with_service(args, CoreService::new(host), writer, |_| {})
            });
            wait.arrive();
            if !quiet {
                wait_for(|| {
                    let bytes = writer.0.lock().unwrap();
                    let output = String::from_utf8_lossy(&bytes);
                    let progress = if json {
                        output
                            .lines()
                            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                            .any(|record| {
                                record["kind"] == "event"
                                    && record["phase"] == "read"
                                    && record["operation_id"] == "1"
                                    && record["progress"]["completed_bytes"] == "1024"
                                    && record["progress"]["total_bytes"] == "1024"
                            })
                    } else {
                        output.contains("read: 1024 / 1024 bytes")
                    };
                    progress.then_some(())
                });
            }
            wait.release();
            assert_eq!(run.join().unwrap(), 4);
            assert_eq!(
                fs::read(output.join("lun0/boot.bin.partial")).unwrap(),
                [7; 1024]
            );
            let bytes = writer.0.lock().unwrap();
            let text = String::from_utf8_lossy(&bytes);
            if json {
                let frames: Vec<Value> = text
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(frames.len() == 1, quiet);
                let result = frames.last().unwrap();
                assert_eq!(result["kind"], "result");
                assert!(result["outputs"].as_array().unwrap().iter().any(|entry| {
                    entry["state"] == "partial"
                        && entry["path"]["display"]
                            .as_str()
                            .unwrap()
                            .ends_with("boot.bin.partial")
                }));
            } else {
                assert_eq!(text.contains("read: 1024 / 1024 bytes"), !quiet);
                assert!(
                    text.lines()
                        .any(|line| line.starts_with("Output partial (partition)")
                            && line.contains("boot.bin.partial"))
                );
            }
        }
    }
}

#[test]
fn wait_timeout_and_indefinite_wait_use_the_real_signal_relay() {
    use oplus_edl_cli::run_with_service;
    use oplus_edl_core::{HostServices, model::Backend, test_support::service_with_stop_clock};
    use std::{sync::mpsc, time::Duration};
    use stop_support::ManualClock;

    let temp = tempfile::tempdir().unwrap();
    let loader = temp.path().join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    for (wait, code) in [("2", 2), ("0", 130)] {
        let clock = ManualClock::default();
        let (enumerated, polled) = mpsc::sync_channel(1);
        let host = HostServices::default().with_backend(
            Backend::Usb,
            {
                let clock = clock.clone();
                move || {
                    enumerated.try_send(()).unwrap();
                    if wait == "2" {
                        clock.set(3);
                        Ok(vec!["late-device".into()])
                    } else {
                        Ok(vec![])
                    }
                }
            },
            |_, _| panic!("expired or interrupted discovery opened a device"),
        );
        let cli = Cli::parse_from([
            "oplus-edl",
            "partitions",
            "--loader",
            loader.to_str().unwrap(),
            "--wait-timeout",
            wait,
            "--json",
            "-q",
        ]);
        let writer = Capture::default();
        let service = service_with_stop_clock(host, clock.clock());
        assert_eq!(
            run_with_service(cli, service, writer.clone(), move |relay| {
                polled.recv_timeout(Duration::from_secs(3)).unwrap();
                if wait == "0" {
                    relay();
                }
            }),
            code
        );
        let terminal = writer.terminal();
        assert_eq!(terminal["session_generation"], Value::Null);
        if wait == "0" {
            assert_eq!(terminal["stop_reason"], "signal");
        } else {
            assert!(
                terminal["errors"][0]["message"]
                    .as_str()
                    .unwrap()
                    .contains("timed out waiting")
            );
        }
    }
}

#[cfg(feature = "cloud")]
#[test]
fn cloud_fetch_rejects_legacy_identity_creation_before_acquisition() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("legacy.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "enrollment_ticket": "offline-test",
            "query": {"platform": "qualcomm", "scheme": "DIGEST", "device_code": "",
                "pk_hash_sha256": "", "pk_hash_sha384": "", "msm_id": "01234567"}
        }))
        .unwrap(),
    )
    .unwrap();
    let out = temp.path().join("assets");
    let (code, result) = invoke(&[
        "cloud",
        "fetch",
        "--config",
        config.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--create-identity",
    ]);
    assert_eq!(code, 2);
    assert_eq!(result["operation"], "cloud_fetch");
    assert_eq!(result["session_generation"], Value::Null);
    assert!(
        result["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--create-identity requires SWM")
    );
    assert!(!out.exists());
}
