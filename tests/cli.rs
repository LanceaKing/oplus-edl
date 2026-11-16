use clap::{CommandFactory, Parser};
use oplus_edl::{
    cli::{Cli, Command},
    device,
    output::{Item, Report, Status},
};
use serde_json::Value;
use std::process::Command as Process;

fn invoke(args: &[&str]) -> (i32, Value) {
    let result = Process::new(env!("CARGO_BIN_EXE_oplus-edl"))
        .args(args)
        .arg("--json")
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
#[test]
fn clap_contract_and_five_entries() {
    Cli::command().debug_assert();
    for command in ["unlock", "lock", "backup", "restore", "flash"] {
        assert!(Cli::command().find_subcommand(command).is_some());
    }
    let cli = Cli::try_parse_from([
        "oplus-edl",
        "restore",
        "--image",
        "a.bin",
        "--partition",
        "boot",
    ])
    .unwrap();
    assert_eq!(cli.command.verify(), Some("best-effort"));
    assert!(
        Cli::try_parse_from([
            "oplus-edl",
            "restore",
            "--from",
            "x",
            "--image",
            "a.bin",
            "--partition",
            "boot"
        ])
        .is_err()
    );
    assert!(Cli::try_parse_from(["oplus-edl", "restore", "--image", "a.bin"]).is_err());
}
#[test]
fn unavailable_workflows_exit_before_hardware_or_loader_access() {
    for args in [vec!["unlock"], vec!["lock"]] {
        let (code, result) = invoke(&args);
        assert_eq!(code, 3);
        assert_eq!(result["status"], "unsupported");
        assert!(result["target"].is_null());
        assert_eq!(result["items"], serde_json::json!([]));
        assert_eq!(result["candidates"], serde_json::json!([]));
        for key in [
            "command",
            "verify_mode",
            "target",
            "coverage",
            "candidates",
            "status",
            "items",
            "errors",
        ] {
            assert!(result.get(key).is_some());
        }
    }
}
#[test]
fn invalid_inputs_and_phase_boundary_do_not_open_hardware() {
    for args in [
        vec!["restore", "--from", "/does-not-exist"],
        vec!["flash", "--program", "/does-not-exist"],
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
            "boot",
            "--loader",
            "/does-not-exist",
        ],
        vec!["restore", "--from", "x", "--image", "y"],
    ] {
        let (code, value) = invoke(&args);
        assert_eq!(code, 2, "{args:?}");
        assert_eq!(value["status"], "input_error");
        assert!(value["target"].is_null());
    }
    let (code, _) = invoke(&["backup", "--output", "x", "--device", "not-opened"]);
    assert_eq!(code, 2);
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
        let cli = Cli::try_parse_from(["oplus-edl", "--memory", memory, "unlock"]).unwrap();
        let config = device::configuration(&cli).unwrap();
        assert_eq!(config.storage_sector_size, sector);
        assert!(!config.bypass_storage);
        assert!(config.quiet);
    }
    let cli = Cli::try_parse_from(["oplus-edl", "--sector-size", "1024", "unlock"]).unwrap();
    assert!(device::configuration(&cli).is_err());
}
#[test]
fn result_priority_retains_all_facts_and_decimal_byte_strings() {
    let mut report = Report::new(Some("backup"), None);
    for status in [
        Status::VerificationIncomplete,
        Status::Unsupported,
        Status::InputError,
        Status::ExecutionFailed,
        Status::UnknownWrite,
    ] {
        let mut item = Item::read(0, Some("boot".into()), u64::MAX, 4096);
        item.fail(status, "test");
        report.items.push(item);
        report.summarize();
        assert_eq!(report.status, status);
    }
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["items"][0]["start_byte"], u64::MAX.to_string());
    assert_eq!(value["items"].as_array().unwrap().len(), 5);
    for (i, item) in value["items"].as_array().unwrap().iter().enumerate() {
        assert_eq!(item["id"], i + 1);
    }
    assert_eq!(Report::new(Some("backup"), None).status.exit_code(), 0);
}
#[test]
fn typed_mutation_failures_map_without_error_string_matching() {
    use qdl::types::MutationFailure;
    for (error, expected, status) in [
        (
            MutationFailure::not_sent(anyhow::anyhow!("x")),
            "not_attempted",
            Status::ExecutionFailed,
        ),
        (
            MutationFailure::rejected(anyhow::anyhow!("x")),
            "failed",
            Status::ExecutionFailed,
        ),
        (
            MutationFailure::unknown(anyhow::anyhow!("x")),
            "unknown",
            Status::UnknownWrite,
        ),
    ] {
        let mut item = Item::read(0, None, 0, 512);
        item.operation = "program".into();
        item.mutation_failure(&error);
        assert_eq!(item.write, expected);
        assert_eq!(item.execution, "failed");
        assert_eq!(item.verification, "not_applicable");
        assert_eq!(item.errors[0].category, status);
    }
}
#[test]
fn removed_features_are_not_accepted() {
    for option in ["--resume", "--plan-out", "--apply-plan", "--approve-plan"] {
        assert!(Cli::try_parse_from(["oplus-edl", "flash", option]).is_err());
    }
    assert!(matches!(
        Cli::try_parse_from(["oplus-edl", "flash"]).unwrap().command,
        Command::Flash { .. }
    ));
}
#[test]
fn phase_four_preflight_json_remains_offline_and_reports_unsupported_features() {
    let (code, value) = invoke(&["flash"]);
    assert_eq!(code, 3);
    assert_eq!(value["status"], "unsupported");
    let tmp = tempfile::tempdir().unwrap();
    let patch = tmp.path().join("patch.xml");
    std::fs::write(&patch, r#"<data><patch filename="DISK" SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="0" start_sector="0" byte_offset="0" size_in_bytes="4" value="1"/></data>"#).unwrap();
    let (code, value) = invoke(&[
        "flash",
        "--patch",
        patch.to_str().unwrap(),
        "--verify",
        "required",
    ]);
    assert_eq!(code, 3);
    assert_eq!(value["items"][0]["execution"], "not_attempted");
    assert!(value["target"].is_null());
    assert_eq!(value["candidates"], serde_json::json!([]));
    let (code, value) = invoke(&[
        "restore",
        "--image",
        "missing",
        "--partition",
        "a",
        "--partition",
        "b",
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
            "--memory",
            "emmc",
            "flash",
            "--program",
            raw.to_str().unwrap(),
            "--patch",
            patch.to_str().unwrap(),
            "--verify",
            mode,
        ]);
        let items:Vec<_>=[("program",&raw,"empty filename"),("patch",&patch,"local file patch skipped: boot.bin")].into_iter().enumerate().map(|(n,(op,path,reason))|serde_json::json!({
            "id":n+1,"operation":op,"lun":0,"partition":null,"source":path.to_str().unwrap(),"xml_node":1,
            "start_byte":"32768","source_bytes":null,"partition_bytes":null,"transfer_bytes":null,"transferred_bytes":null,
            "execution":"skipped","write":"not_applicable","verification":"not_applicable","ranges":[],"reason":reason,"errors":[]
        })).collect();
        assert_eq!(code, 3);
        assert_eq!(
            result,
            serde_json::json!({"command":"flash","verify_mode":mode,"target":null,
            "coverage":{"limited":true,"reason":"storage scope not discovered"},"candidates":[],"status":"unsupported","items":items,
            "errors":[{"category":"unsupported","message":"no executable storage operations"}]})
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
            "--memory",
            "emmc",
            "flash",
            "--program",
            xml.to_str().unwrap(),
        ]);
        assert_eq!(actual, code);
        assert_eq!(r["status"], status);
        assert_eq!(r["errors"][0]["category"], status);
        assert_eq!(r["items"], serde_json::json!([]));
        assert_eq!(r["candidates"], serde_json::json!([]));
        assert!(r["target"].is_null());
        let message = r["errors"][0]["message"].as_str().unwrap().to_owned();
        assert!(!message.is_empty());
        assert_eq!(
            r,
            serde_json::json!({"command":"flash","verify_mode":"best-effort","target":null,
            "coverage":{"limited":true,"reason":"storage scope not discovered"},"candidates":[],"status":status,"items":[],
            "errors":[{"category":status,"message":message}]})
        );
    }
}

#[test]
fn firmware_length_error_identifies_the_xml_source_and_target_without_hardware() {
    let tmp = tempfile::tempdir().unwrap();
    let xml = tmp.path().join("rawprogram0.xml");
    std::fs::write(tmp.path().join("vbmeta_system.img"), [0; 8192]).unwrap();
    std::fs::write(&xml, r#"<data><program filename="vbmeta_system.img" label="vbmeta_system_a" SECTOR_SIZE_IN_BYTES="4096" physical_partition_number="0" start_sector="64" num_partition_sectors="16"/></data>"#).unwrap();
    let (code, result) = invoke(&["flash", "--program", xml.to_str().unwrap()]);
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
fn cloud_flash_flags_require_output_and_exclude_local_auth_inputs() {
    assert!(Cli::try_parse_from(["oplus-edl", "flash", "--cloud-fetch-only"]).is_err());
    assert!(Cli::try_parse_from(["oplus-edl", "flash", "--cloud-config", "cloud.json"]).is_err());
    assert!(
        Cli::try_parse_from([
            "oplus-edl",
            "flash",
            "--cloud-config",
            "cloud.json",
            "--cloud-output",
            "assets",
            "--loader",
            "local.elf"
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "oplus-edl",
            "flash",
            "--cloud-config",
            "cloud.json",
            "--cloud-output",
            "assets",
            "--cloud-fetch-only"
        ])
        .is_ok()
    );
}

#[test]
fn flash_waits_by_default_and_accepts_a_bounded_wait() {
    let cli = Cli::try_parse_from(["oplus-edl", "flash"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Flash {
            wait_timeout: 0,
            ..
        }
    ));
    let cli = Cli::try_parse_from(["oplus-edl", "flash", "--wait-timeout", "30"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Flash {
            wait_timeout: 30,
            ..
        }
    ));
    assert!(Cli::try_parse_from(["oplus-edl", "flash", "--wait-timeout", "-1"]).is_err());
}
