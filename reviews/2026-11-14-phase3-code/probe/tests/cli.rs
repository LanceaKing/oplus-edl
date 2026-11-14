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
    for args in [
        vec!["unlock"],
        vec!["lock"],
        vec!["restore", "--from", "/does-not-exist"],
        vec!["flash", "--program", "/does-not-exist"],
    ] {
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
