use oplus_edl_core::result::{ExecutionState, WriteState};
#[path = "../../oplus-edl-core/tests/support/cloud_fixture.rs"]
mod cloud_fixture;
mod support;

use clap::Parser;
use cloud_fixture::CloudFixture;
use oplus_edl_cli::{OutputMode, OutputWriter, args::Cli, run_with_service};
use oplus_edl_core::{
    CoreService, HostServices, OperationResult, StopReason, SubmitError,
    model::{Backend, Decision, Verify},
    result::{SessionDisposition, Status},
    service::Slot,
    test_support::{loopback_service, service_with_stop_clock},
};
use oplus_edl_desktop::app::{ActionError, DesktopApp, FlashKind, Mode, RestoreKind};
use serde_json::Value;
use std::{
    fs,
    io::{self, Cursor, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use support::{
    app_with_loader, connect, inspect, package_fixture, protocol::*, raw_read, result, summary,
};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn cli(
    args: &[&str],
    service: CoreService,
    quiet: bool,
    register: impl FnOnce(Arc<dyn Fn() + Send + Sync>),
) -> (i32, Vec<Value>) {
    let parsed = Cli::parse_from(
        std::iter::once("oplus-edl")
            .chain(args.iter().copied())
            .chain(std::iter::once("--json"))
            .chain(quiet.then_some("-q")),
    );
    let stdout = Capture::default();
    let code = run_with_service(parsed, service, stdout.clone(), register);
    let bytes = stdout.0.lock().unwrap();
    let records: Vec<Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.last().unwrap()["kind"], "result");
    assert!(
        records[..records.len() - 1]
            .iter()
            .all(|record| record["kind"] == "event")
    );
    (code, records)
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn actual_trace(trace: &Trace) -> Vec<Vec<u8>> {
    trace.lock().unwrap().writes.clone()
}

/// Setup inspection is an independent GUI action. Both adapters' operation traces retain
/// every command from the initialization exchange and the operation being compared.
fn trace_for_operation(
    trace: &Trace,
    before_inspection: usize,
    after_inspection: usize,
) -> Vec<Vec<u8>> {
    let trace = trace.lock().unwrap();
    trace.writes[..before_inspection]
        .iter()
        .chain(&trace.writes[after_inspection..])
        .cloned()
        .collect()
}

fn gui_wire(report: &OperationResult, opened: Option<&OperationResult>) -> Value {
    let mut combined = report.clone();
    if let Some(opened) = opened {
        assert!(report.session.completed_initialization_steps.is_empty());
        assert_eq!(report.session.generation, opened.session.generation);
        combined.session.completed_initialization_steps =
            opened.session.completed_initialization_steps.clone();
    }
    let mut output = Vec::new();
    OutputWriter::new(
        OutputMode {
            json: true,
            quiet: true,
        },
        &mut output,
    )
    .finish(&combined)
    .unwrap();
    serde_json::from_slice(&output).unwrap()
}

fn compare(cli_result: &Value, mut desktop: Value, gui_stop: Option<StopReason>) {
    let mut cli_result = cli_result.clone();
    for key in ["operation_id", "session_generation"] {
        assert_eq!(
            cli_result[key].is_null(),
            desktop[key].is_null(),
            "identity presence for {key}"
        );
        cli_result[key] = Value::Null;
        desktop[key] = Value::Null;
    }
    cli_result["seq"] = Value::Null;
    desktop["seq"] = Value::Null;
    if cli_result["session"]["reason"] == "cli_finished" {
        assert_eq!(cli_result["session"]["disposition"], "closed");
        assert_eq!(desktop["session"]["disposition"], "reusable");
        cli_result["session"]["disposition"] = Value::String("reusable".into());
        cli_result["session"]["reason"] = Value::Null;
    }
    if let Some(reason) = gui_stop {
        assert_eq!(desktop["stop_reason"], serde_json::json!(reason));
        assert_eq!(cli_result["stop_reason"], "signal");
        desktop["stop_reason"] = serde_json::json!("signal");
    }
    assert_eq!(cli_result, desktop);
}

fn device_args<'a>(command: &[&'a str], loader: &'a Path, memory: &'a str) -> Vec<&'a str> {
    let mut args = command.to_vec();
    args.extend([
        "--loader",
        path(loader),
        "--memory",
        memory,
        "--device",
        "scripted",
    ]);
    args
}

#[test]
fn device_discovery_routes_the_same_backend_in_both_real_adapters() {
    for (backend, option, supported) in [
        (Backend::Usb, None, true),
        (Backend::Usb, Some("usb"), true),
        (Backend::Serial, Some("serial"), true),
        (Backend::Serial, Some("serial"), false),
    ] {
        let build_host = || {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let mut host = HostServices::default();
            for provider in [Backend::Usb, Backend::Serial] {
                if provider == Backend::Serial && !supported {
                    continue;
                }
                let calls = calls.clone();
                host = host.with_backend(
                    provider,
                    move || {
                        calls.lock().unwrap().push(provider);
                        Ok(vec![format!("{}:device", provider.as_str())])
                    },
                    |_, _| panic!("discovery opened a channel"),
                );
            }
            (CoreService::new(host), calls)
        };
        let (cli_service, cli_calls) = build_host();
        let (gui_service, gui_calls) = build_host();
        let mut args = vec!["devices"];
        if let Some(option) = option {
            args.extend(["--backend", option]);
        }
        let (code, records) = cli(&args, cli_service, true, |_| {});
        let mut app = DesktopApp::with_service(gui_service);
        let mut inputs = app.inputs().clone();
        inputs.backend = option.map(|_| backend);
        app.update_inputs(inputs);
        if supported {
            let id = app.discover().unwrap();
            let gui = result(&mut app, Slot::Device, id);
            assert_eq!(code, 0);
            compare(records.last().unwrap(), gui_wire(&gui, None), None);
            assert_eq!(*cli_calls.lock().unwrap(), [backend]);
            assert_eq!(*gui_calls.lock().unwrap(), [backend]);
        } else {
            assert_eq!(code, 3);
            assert_eq!(records.last().unwrap()["outcome"], "unsupported");
            assert_eq!(
                app.discover(),
                Err(ActionError::Service(SubmitError::Unsupported))
            );
            assert!(cli_calls.lock().unwrap().is_empty());
            assert!(gui_calls.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn one_cli_inspect_matches_desktop_connect_then_inspect() {
    let temp = tempfile::tempdir().unwrap();
    let build = || {
        let mut script = initialization(false);
        script.push(response(STORAGE_ACK));
        script.extend(gpt_script(&disk(512, &["boot", "system"]), 512));
        scripted_host(script)
    };
    let (gui_host, gui_trace) = build();
    let mut app = app_with_loader(CoreService::new(gui_host), temp.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.lun_scope.clear();
    app.update_inputs(inputs);
    let initialized = connect(&mut app);
    let inspected = inspect(&mut app);
    let (cli_host, cli_trace) = build();
    let (code, records) = cli(
        &device_args(&["partitions"], &app.inputs().loader, "emmc"),
        CoreService::new(cli_host),
        true,
        |_| {},
    );
    assert_eq!(code, 0);
    compare(
        records.last().unwrap(),
        gui_wire(&inspected, Some(&initialized)),
        None,
    );
    assert_eq!(actual_trace(&cli_trace), actual_trace(&gui_trace));
    assert!(!gui_trace.lock().unwrap().closed);
    assert!(cli_trace.lock().unwrap().closed);
}

#[test]
fn backup_plans_read_the_same_bytes_and_publish_the_same_outputs() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("backup");
    let boot = [41; 1024];
    let mock_disk = disk(512, &["boot", "other"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&mock_disk[512..])).unwrap();
    let build = |with_inspection| {
        let mut script = initialization(false);
        if with_inspection {
            script.extend(gpt_script(&mock_disk, 512));
        }
        script.extend(gpt_script(&mock_disk, 512));
        script.extend(raw_read(
            &mock_disk[(header.last_usable_lba as usize + 1) * 512..],
        ));
        script.extend(raw_read(&boot));
        if with_inspection {
            script.extend(gpt_script(&mock_disk, 512));
        }
        scripted_host(script)
    };
    let (gui_host, gui_trace) = build(true);
    let mut app = app_with_loader(CoreService::new(gui_host), root.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.backup.output = output.clone();
    app.update_inputs(inputs);

    let (cli_host, cli_trace) = build(false);
    let args = device_args(
        &["backup", "--output", path(&output), "--partition", "0:boot"],
        &app.inputs().loader,
        "emmc",
    );
    let (code, records) = cli(&args, CoreService::new(cli_host), true, |_| {});
    assert_eq!(code, 0);
    let saved = root.path().join("cli-backup");
    fs::rename(&output, &saved).unwrap();

    let initialized = connect(&mut app);
    let before_inspection = actual_trace(&gui_trace).len();
    inspect(&mut app);
    let after_inspection = actual_trace(&gui_trace).len();
    let key = app.snapshot().unwrap().partitions[0].clone();
    assert!(app.select_partition(&key, true));
    let id = app.start().unwrap();
    let report = result(&mut app, Slot::Device, id);
    compare(
        records.last().unwrap(),
        gui_wire(&report, Some(&initialized)),
        None,
    );
    assert_eq!(
        trace_for_operation(&gui_trace, before_inspection, after_inspection),
        actual_trace(&cli_trace)
    );
    for file in [
        "lun0/boot.bin",
        "lun0/.gpt/primary.bin",
        "lun0/.gpt/backup.bin",
    ] {
        assert_eq!(
            fs::read(output.join(file)).unwrap(),
            fs::read(saved.join(file)).unwrap()
        );
    }
    assert!(!output.join("lun0/other.bin").exists());
    let repeated = inspect(&mut app);
    assert_eq!(repeated.session.generation, initialized.session.generation);
    assert!(repeated.session.completed_initialization_steps.is_empty());
    assert_eq!(
        wire_commands(&gui_trace)
            .iter()
            .filter(|command| command.name == "configure")
            .count(),
        1
    );
}

#[test]
fn image_and_directory_restore_keep_gpt_preflight_and_write_order() {
    for kind in [RestoreKind::Image, RestoreKind::Directory] {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("backup");
        fs::create_dir_all(folder.join("lun0")).unwrap();
        let image = folder.join("lun0/boot.bin");
        let payload = [32; 512];
        fs::write(&image, payload).unwrap();
        let mock_disk = disk(512, &["boot", "other"]);
        let build = |with_inspection| {
            let mut script = initialization(false);
            if with_inspection {
                script.extend(gpt_script(&mock_disk, 512));
            }
            script.extend(gpt_script(&mock_disk, 512));
            script.extend([response(RAW_ACK), response(ACK)]);
            script.extend(raw_read(&payload));
            if with_inspection {
                script.extend(gpt_script(&mock_disk, 512));
            }
            scripted_host(script)
        };
        let (gui_host, gui_trace) = build(true);
        let mut app = app_with_loader(CoreService::new(gui_host), root.path(), 512, false);
        let mut inputs = app.inputs().clone();
        inputs.restore.kind = kind;
        inputs.restore.image = image.clone();
        inputs.restore.directory = folder.clone();
        inputs.restore.all = kind == RestoreKind::Directory;
        inputs.verify = Verify::Required;
        app.update_inputs(inputs);
        app.set_mode(Mode::Restore);
        let initialized = connect(&mut app);
        let before = actual_trace(&gui_trace).len();
        inspect(&mut app);
        let after = actual_trace(&gui_trace).len();
        if kind == RestoreKind::Image {
            let target = app.snapshot().unwrap().partitions[0].clone();
            app.select_partition(&target, true);
        }
        let id = app.start().unwrap();
        let write = summary(&mut app);
        assert_eq!(write.items.len(), 1);
        assert_eq!(write.operation_id, id);
        app.decide(write.id, Decision::Approve).unwrap();
        let report = result(&mut app, Slot::Device, id);
        assert_eq!(report.status, Status::Success);
        assert!(
            app.snapshot().is_none(),
            "writes invalidate prior GPT views"
        );

        let cli_path = match kind {
            RestoreKind::Image => vec![
                "restore",
                "image",
                "--image",
                path(&image),
                "--partition",
                "0:boot",
            ],
            RestoreKind::Directory => vec![
                "restore",
                "directory",
                "--from",
                path(&folder),
                "--all",
                "--lun",
                "0",
            ],
        };
        let mut args = device_args(&cli_path, &app.inputs().loader, "emmc");
        args.extend(["--yes", "--verify", "required"]);
        let (cli_service, cli_trace) = build(false);
        let (code, records) = cli(&args, CoreService::new(cli_service), true, |_| {});
        assert_eq!(code, 0);
        compare(
            records.last().unwrap(),
            gui_wire(&report, Some(&initialized)),
            None,
        );
        assert_eq!(
            trace_for_operation(&gui_trace, before, after),
            actual_trace(&cli_trace)
        );
        let repeated = inspect(&mut app);
        assert_eq!(repeated.session.generation, initialized.session.generation);
        assert!(repeated.session.completed_initialization_steps.is_empty());
        assert_eq!(
            wire_commands(&gui_trace)
                .iter()
                .filter(|command| command.name == "configure")
                .count(),
            1
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlashCase {
    Match,
    Mismatch,
    Refused,
    LostAck,
    Changed,
    Stopped,
    Reboot,
}

fn program(root: &Path, sector: usize) -> (std::path::PathBuf, std::path::PathBuf) {
    let image = root.join("boot.bin");
    let program = root.join("program.xml");
    fs::write(&image, vec![0x34; sector]).unwrap();
    fs::write(&program, format!(r#"<data><program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="{sector}" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#)).unwrap();
    (image, program)
}

#[test]
fn xml_program_verification_source_change_unknown_ack_and_stop_have_equivalent_results() {
    for (case, verify, exit, outcome) in [
        (FlashCase::Match, Verify::Required, 0, Status::Success),
        (
            FlashCase::Mismatch,
            Verify::Required,
            4,
            Status::ExecutionFailed,
        ),
        (
            FlashCase::Refused,
            Verify::BestEffort,
            5,
            Status::VerificationIncomplete,
        ),
        (FlashCase::LostAck, Verify::None, 6, Status::UnknownWrite),
        (FlashCase::Changed, Verify::None, 7, Status::InputChanged),
        (FlashCase::Stopped, Verify::None, 130, Status::Stopped),
        (FlashCase::Reboot, Verify::None, 0, Status::Success),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (image, xml) = program(root.path(), 512);
        let build = |desktop| {
            let mut script = initialization(false);
            script.push(response(RAW_ACK));
            let mut pause = None;
            match case {
                FlashCase::LostAck => script.push(expect_response(Input::Eof)),
                FlashCase::Changed => {
                    let image = image.clone();
                    script.push(on_refill(Access::Response, bytes(ACK), move || {
                        fs::OpenOptions::new()
                            .write(true)
                            .open(image)
                            .unwrap()
                            .set_len(1024)
                            .unwrap();
                    }));
                }
                FlashCase::Stopped => {
                    let (ack, pending) = pause_response(bytes(ACK));
                    script.push(ack);
                    pause = Some(pending);
                }
                _ => script.push(response(ACK)),
            }
            match case {
                FlashCase::Match => script.extend(raw_read(&[0x34; 512])),
                FlashCase::Mismatch => script.extend(raw_read(&[0x35; 512])),
                FlashCase::Refused => script.push(response(NAK)),
                FlashCase::Reboot => script.push(response(ACK)),
                _ => {}
            }
            if desktop && case == FlashCase::Mismatch {
                script.extend(gpt_script(&disk(512, &["boot"]), 512));
            }
            let (host, trace) = scripted_host(script);
            (CoreService::new(host), trace, pause)
        };

        let (gui_service, gui_trace, pause) = build(true);
        let mut app = app_with_loader(gui_service, root.path(), 512, false);
        let mut inputs = app.inputs().clone();
        inputs.flash.programs = vec![xml.clone()];
        inputs.verify = verify;
        inputs.flash.reboot = matches!(case, FlashCase::Reboot | FlashCase::Stopped);
        app.update_inputs(inputs);
        app.set_mode(Mode::Flash);
        let initialized = connect(&mut app);
        let id = app.start().unwrap();
        let prepared = summary(&mut app);
        assert!(prepared.package.is_none());
        assert!(
            prepared
                .coverage
                .reason
                .as_deref()
                .unwrap()
                .contains("matching unavailable")
        );
        app.decide(prepared.id, Decision::Approve).unwrap();
        if let Some(pause) = pause {
            pause.arrive();
            app.stop(Slot::Device);
            pause.release();
        }
        let desktop = result(&mut app, Slot::Device, id);
        assert_eq!(desktop.status, outcome);
        if matches!(case, FlashCase::Mismatch | FlashCase::Refused) {
            assert_eq!(desktop.session.disposition, SessionDisposition::Reusable);
            assert!(app.session().is_some());
        }
        if matches!(case, FlashCase::LostAck | FlashCase::Reboot) {
            assert!(app.session().is_none());
            assert!(gui_trace.lock().unwrap().closed);
        }

        fs::write(&image, [0x34; 512]).unwrap();
        let (cli_service, cli_trace, pause) = build(false);
        let mut args = device_args(
            &["flash", "xml", "--program", path(&xml)],
            &app.inputs().loader,
            "emmc",
        );
        args.extend(["--yes", "--verify", verify.as_str()]);
        if matches!(case, FlashCase::Reboot | FlashCase::Stopped) {
            args.push("--reboot");
        }
        let (code, records) = cli(&args, cli_service, true, move |relay| {
            if let Some(pause) = pause {
                std::thread::spawn(move || {
                    pause.arrive();
                    relay();
                    pause.release();
                });
            }
        });
        assert_eq!(code, exit, "{records:?}");
        compare(
            records.last().unwrap(),
            gui_wire(&desktop, Some(&initialized)),
            (case == FlashCase::Stopped).then_some(StopReason::User),
        );
        assert_eq!(actual_trace(&gui_trace), actual_trace(&cli_trace));
        if case == FlashCase::Mismatch {
            let repeated = inspect(&mut app);
            assert_eq!(repeated.session.generation, initialized.session.generation);
            assert!(repeated.session.completed_initialization_steps.is_empty());
            assert_eq!(
                wire_commands(&gui_trace)
                    .iter()
                    .filter(|command| command.name == "configure")
                    .count(),
                1
            );
        }
    }
}

#[test]
fn boot_lun_rejection_stays_read_only_in_both_adapters() {
    let root = tempfile::tempdir().unwrap();
    let (_, xml) = program(root.path(), 4096);
    let build = || {
        let mut script = initialization(true);
        script.extend([
            response(b"<data><log value=\"INFO: UFS Boot LUN ID: = 0x0\"/></data>"),
            response(ACK),
        ]);
        scripted_host(script)
    };
    let (host, gui_trace) = build();
    let mut app = app_with_loader(CoreService::new(host), root.path(), 4096, true);
    let mut inputs = app.inputs().clone();
    inputs.flash.programs = vec![xml.clone()];
    inputs.flash.boot_lun = Some(1);
    inputs.flash.reboot = true;
    inputs.verify = Verify::None;
    app.update_inputs(inputs);
    app.set_mode(Mode::Flash);
    let initialized = connect(&mut app);
    let id = app.start().unwrap();
    let desktop = result(&mut app, Slot::Device, id);
    assert_eq!(desktop.status, Status::InputError);
    assert!(desktop.errors[0].message.contains("not a UFS boot LUN"));
    assert!(app.pending_write().is_none());

    let args = device_args(
        &[
            "flash",
            "xml",
            "--program",
            path(&xml),
            "--set-bootable",
            "1",
            "--reboot",
            "--yes",
            "--verify",
            "none",
            "--signed-digest",
            path(app.inputs().signed_digest.as_ref().unwrap()),
            "--digest-signature",
            path(app.inputs().digest_signature.as_ref().unwrap()),
        ],
        &app.inputs().loader,
        "ufs",
    );
    let (host, cli_trace) = build();
    let (code, records) = cli(&args, CoreService::new(host), true, |_| {});
    assert_eq!(code, 2);
    compare(
        records.last().unwrap(),
        gui_wire(&desktop, Some(&initialized)),
        None,
    );
    assert_eq!(actual_trace(&cli_trace), actual_trace(&gui_trace));
    let commands = wire_commands(&gui_trace);
    assert_eq!(commands.last().unwrap().name, "getstorageinfo");
    assert!(commands.iter().all(|command| !matches!(
        command.name.as_str(),
        "program" | "patch" | "power" | "setbootablestoragedrive"
    )));
}

#[test]
fn package_preparation_and_reopening_keep_the_project_guard_and_new_consent() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    package_fixture::package_with_programs(dir);
    let out = dir.join("prepared");
    let args = [
        "package",
        "prepare",
        "--package",
        path(dir),
        "--nv-id",
        "123",
        "--output",
        path(&out),
    ];
    let (code, records) = cli(
        &args,
        CoreService::new(HostServices::default()),
        true,
        |_| {},
    );
    assert_eq!(code, 0, "{records:?}");
    assert_eq!(records.last().unwrap()["session_generation"], Value::Null);
    let before = dir.join("cli-prepared");
    fs::rename(&out, &before).unwrap();

    let (host, gui_trace) = scripted_host(package_project_script(23632));
    let mut app = app_with_loader(CoreService::new(host), dir, 4096, false);
    app.set_mode(Mode::Flash);
    let mut inputs = app.inputs().clone();
    inputs.flash.kind = FlashKind::GeneratePackage;
    inputs.flash.package = dir.to_path_buf();
    inputs.flash.nv_id = "123".into();
    inputs.flash.generated = out.clone();
    inputs.verify = Verify::None;
    app.update_inputs(inputs);
    let id = app.prepare_package().unwrap();
    let prepared = result(&mut app, Slot::Offline, id);
    assert!(app.session().is_none());
    assert!(actual_trace(&gui_trace).is_empty());
    compare(records.last().unwrap(), gui_wire(&prepared, None), None);
    let artifacts = |dir: &Path| {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let files = artifacts(&out);
    for (name, initial) in artifacts(&before) {
        if name == "selection.json" || name.to_string_lossy().ends_with(".xml") {
            // Attribute order is presentation. Bindings and ordered operations are checked on reopen.
            continue;
        }
        assert_eq!(files[&name], initial);
    }

    let mut inputs = app.inputs().clone();
    inputs.flash.kind = FlashKind::ReopenPackage;
    inputs.flash.prepared = out.clone();
    inputs.flash.reboot = true;
    app.update_inputs(inputs);
    let initialized = connect(&mut app);
    let id = app.start().unwrap();
    let desktop = result(&mut app, Slot::Device, id);
    assert_eq!(desktop.status, Status::InputError);
    assert!(
        desktop.errors[0]
            .message
            .contains("23632 does not match package project 23631")
    );
    assert_eq!(desktop.items.len(), 13);
    assert!(
        desktop
            .items
            .iter()
            .all(|item| item.operation == "read" && item.execution == ExecutionState::Done)
    );
    assert!(app.pending_write().is_none());

    let (host, cli_trace) = scripted_host(package_project_script(23632));
    let args = device_args(
        &[
            "flash",
            "package",
            "--package",
            path(dir),
            "--nv-id",
            "123",
            "--prepared",
            path(&out),
            "--yes",
            "--verify",
            "none",
            "--reboot",
        ],
        &app.inputs().loader,
        "ufs",
    );
    let (code, records) = cli(&args, CoreService::new(host), true, |_| {});
    assert_eq!(code, 2);
    compare(
        records.last().unwrap(),
        gui_wire(&desktop, Some(&initialized)),
        None,
    );
    assert_eq!(actual_trace(&gui_trace), actual_trace(&cli_trace));
    assert_eq!(artifacts(&out), files);

    let (host, matched_trace) = scripted_host(package_project_script(23631));
    let mut matched = app_with_loader(CoreService::new(host), dir, 4096, false);
    matched.update_inputs(app.inputs().clone());
    matched.set_mode(Mode::Flash);
    connect(&mut matched);
    let id = matched.start().unwrap();
    let consent = summary(&mut matched);
    assert_eq!(consent.operation_id, id);
    assert_eq!(consent.package.as_ref().unwrap().origin.project, 23631);
    assert!(
        wire_commands(&matched_trace)
            .iter()
            .all(|command| matches!(command.name.as_str(), "configure" | "read"))
    );
    matched.decide(consent.id, Decision::Decline).unwrap();
    let rejected = result(&mut matched, Slot::Device, id);
    assert_eq!(rejected.status, Status::InputError);
    assert_eq!(
        rejected
            .items
            .iter()
            .filter(|item| item.write == WriteState::Acknowledged)
            .count(),
        0
    );
    assert_eq!(artifacts(&out), files);
}

#[test]
fn malformed_prepared_bindings_fail_before_package_preflight_in_both_adapters() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    package_fixture::package_with_programs(dir);
    let out = dir.join("out");
    let other = dir.join("other");
    oplus_edl_core::package::prepare(dir, "123", &out, None, 4096).unwrap();
    oplus_edl_core::package::prepare(dir, "123", &other, None, 4096).unwrap();
    let manifest = out.join("selection.json");
    let evidence = fs::read(&manifest).unwrap();
    let xml = out.join("rawprogram0.xml");
    let valid_xml = fs::read_to_string(&xml).unwrap();
    let relocated = dir.join("relocated");
    let (host, trace) = scripted_host(initialization(false));
    let mut app = app_with_loader(CoreService::new(host), dir, 4096, false);
    app.set_mode(Mode::Flash);
    connect(&mut app);
    let initial_wire = actual_trace(&trace);
    for mode in ["missing", "mixed", "changed", "relocated"] {
        match mode {
            "missing" => fs::remove_file(&manifest).unwrap(),
            "mixed" => {
                fs::copy(other.join("selection.json"), &manifest).unwrap();
            }
            "changed" => fs::write(
                &xml,
                valid_xml.replacen("start_sector=\"0\"", "start_sector=\"1\"", 1),
            )
            .unwrap(),
            _ => fs::rename(&out, &relocated).unwrap(),
        }
        let prepared = if mode == "relocated" {
            &relocated
        } else {
            &out
        };
        let mut inputs = app.inputs().clone();
        inputs.flash.kind = FlashKind::ReopenPackage;
        inputs.flash.package = dir.to_owned();
        inputs.flash.nv_id = "123".into();
        inputs.flash.prepared = prepared.clone();
        inputs.verify = Verify::None;
        app.update_inputs(inputs);
        let id = app.start().unwrap();
        let report = result(&mut app, Slot::Device, id);
        assert_eq!(report.status, Status::InputError, "{mode}: {report:?}");
        assert!(app.pending_write().is_none());
        assert!(app.session().is_some());
        assert_eq!(actual_trace(&trace), initial_wire);

        let host = HostServices::default().with_backend(
            Backend::Usb,
            || panic!("invalid artifact enumerated"),
            |_, _| panic!("invalid artifact opened"),
        );
        let args = device_args(
            &[
                "flash",
                "package",
                "--package",
                path(dir),
                "--nv-id",
                "123",
                "--prepared",
                path(prepared),
                "--yes",
                "--verify",
                "none",
            ],
            &app.inputs().loader,
            "ufs",
        );
        let (code, records) = cli(&args, CoreService::new(host), true, |_| {});
        assert_eq!(code, 2, "{mode}");
        compare(records.last().unwrap(), gui_wire(&report, None), None);
        if mode == "relocated" {
            fs::rename(&relocated, &out).unwrap();
        }
        fs::write(&manifest, &evidence).unwrap();
        fs::write(&xml, &valid_xml).unwrap();
    }
}

#[test]
fn legacy_cloud_fetch_reaches_invalid_ticket_hmac_without_opening_or_publishing() {
    check_cloud_ticket_failure(
        || CloudFixture::start().unwrap(),
        "cloud ticket HMAC mismatch",
        None,
    );
}

#[test]
fn malformed_cloud_ticket_text_stays_out_of_both_frontends() {
    const MARKER: &str = "TEST_PRIVATE_TICKET_FIELD";
    check_cloud_ticket_failure(
        || CloudFixture::start_with_malformed_ticket(MARKER).unwrap(),
        "invalid cloud ticket response",
        Some(MARKER),
    );
}

fn check_cloud_ticket_failure(
    start_fixture: impl Fn() -> CloudFixture,
    expected_error: &str,
    redacted_marker: Option<&str>,
) {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("cloud.json");
    let out = root.path().join("assets");
    cloud_fixture::write_config(&config).unwrap();
    let host = || {
        HostServices::default().with_backend(
            Backend::Usb,
            || panic!("cloud fetch enumerated"),
            |_, _| panic!("cloud fetch opened"),
        )
    };
    let gui_fixture = start_fixture();
    let mut app = DesktopApp::with_service(loopback_service(host(), gui_fixture.address).unwrap());
    let mut inputs = app.inputs().clone();
    inputs.cloud.config = config.clone();
    inputs.cloud.output = out.clone();
    app.update_inputs(inputs);
    let id = app.fetch_cloud().unwrap();
    let desktop = result(&mut app, Slot::Offline, id);
    assert_eq!(gui_fixture.finish().unwrap(), ["enroll", "list", "ticket"]);
    assert_eq!(desktop.status, Status::InputError);
    assert!(desktop.errors[0].message.contains(expected_error));
    assert!(desktop.session.generation.is_none());
    assert!(desktop.outputs.is_empty());
    assert!(!out.exists());

    let cli_fixture = start_fixture();
    let args = [
        "cloud",
        "fetch",
        "--config",
        path(&config),
        "--output",
        path(&out),
    ];
    let (code, records) = cli(
        &args,
        loopback_service(host(), cli_fixture.address).unwrap(),
        false,
        |_| {},
    );
    assert_eq!(cli_fixture.finish().unwrap(), ["enroll", "list", "ticket"]);
    assert_eq!(code, 2);
    let events: Vec<_> = records[..records.len() - 1]
        .iter()
        .map(|record| record["phase"].as_str().unwrap())
        .collect();
    assert_eq!(
        events,
        app.recent_events()
            .map(|event| event.phase)
            .collect::<Vec<_>>()
    );
    assert_eq!(events, ["acquisition"]);
    compare(records.last().unwrap(), gui_wire(&desktop, None), None);
    if let Some(marker) = redacted_marker {
        assert!(
            desktop
                .errors
                .iter()
                .all(|error| !error.message.contains(marker))
        );
        assert!(
            records
                .iter()
                .all(|record| !record.to_string().contains(marker))
        );
    }
    assert!(!out.exists());
}

#[test]
fn a_stopped_in_flight_ack_preserves_typed_expiry_in_both_adapters() {
    let root = tempfile::tempdir().unwrap();
    let (_, xml) = program(root.path(), 512);
    let build = || {
        let mut script = initialization(false);
        let (ack, pause) = pause_response(bytes(ACK));
        script.extend([response(RAW_ACK), ack]);
        let (host, trace) = scripted_host(script);
        let clock = ManualClock::default();
        (
            service_with_stop_clock(host, clock.clock()),
            trace,
            clock,
            pause,
        )
    };
    let (service, gui_trace, clock, pause) = build();
    let mut app = app_with_loader(service, root.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.flash.programs = vec![xml.clone()];
    inputs.verify = Verify::None;
    app.update_inputs(inputs);
    app.set_mode(Mode::Flash);
    let initialized = connect(&mut app);
    let id = app.start().unwrap();
    let prepared = summary(&mut app);
    app.decide(prepared.id, Decision::Approve).unwrap();
    pause.arrive();
    app.stop(Slot::Device);
    clock.set(11);
    pause.release();
    let desktop = result(&mut app, Slot::Device, id);
    assert_eq!(desktop.status, Status::UnknownWrite);
    assert!(desktop.items[0].errors[0].cause.is_some());
    assert_eq!(desktop.items[0].transferred_bytes.as_deref(), Some("512"));
    assert!(app.session().is_none());

    let (service, cli_trace, clock, pause) = build();
    let args = device_args(
        &[
            "flash",
            "xml",
            "--program",
            path(&xml),
            "--yes",
            "--verify",
            "none",
        ],
        &app.inputs().loader,
        "emmc",
    );
    let (code, records) = cli(&args, service, true, move |relay| {
        std::thread::spawn(move || {
            pause.arrive();
            relay();
            clock.set(11);
            pause.release();
        });
    });
    assert_eq!(code, 6);
    compare(
        records.last().unwrap(),
        gui_wire(&desktop, Some(&initialized)),
        Some(StopReason::User),
    );
    assert_eq!(actual_trace(&cli_trace), actual_trace(&gui_trace));
}

#[test]
fn reopened_package_rejects_a_range_outside_the_device_lun_in_both_adapters() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let out = dir.join("prepared");
    package_fixture::package_with_programs(dir);
    oplus_edl_core::package::prepare(dir, "123", &out, None, 4096).unwrap();
    let fixture = || {
        let mut script = initialization(false);
        // 256 sectors cannot contain the selected 512-sector super span.
        script.extend(gpt_script(&disk(4096, &[]), 4096));
        scripted_host(script)
    };
    let (host, gui_trace) = fixture();
    let mut app = app_with_loader(CoreService::new(host), dir, 4096, false);
    let mut inputs = app.inputs().clone();
    inputs.flash.kind = FlashKind::ReopenPackage;
    inputs.flash.package = dir.to_path_buf();
    inputs.flash.prepared = out.clone();
    inputs.flash.nv_id = "123".into();
    inputs.verify = Verify::None;
    app.update_inputs(inputs);
    app.set_mode(Mode::Flash);
    let initialized = connect(&mut app);
    let id = app.start().unwrap();
    let desktop = result(&mut app, Slot::Device, id);
    assert_eq!(desktop.status, Status::InputError);
    assert!(
        desktop.errors[0]
            .message
            .contains("package mutation exceeds device LUN 0")
    );
    assert!(app.pending_write().is_none());
    assert!(
        wire_commands(&gui_trace)
            .iter()
            .all(|command| matches!(command.name.as_str(), "configure" | "read"))
    );
    let (host, cli_trace) = fixture();
    let args = device_args(
        &[
            "flash",
            "package",
            "--package",
            path(dir),
            "--nv-id",
            "123",
            "--prepared",
            path(&out),
            "--yes",
            "--verify",
            "none",
        ],
        &app.inputs().loader,
        "ufs",
    );
    let (code, records) = cli(&args, CoreService::new(host), true, |_| {});
    assert_eq!(code, 2);
    compare(
        records.last().unwrap(),
        gui_wire(&desktop, Some(&initialized)),
        None,
    );
    assert_eq!(actual_trace(&gui_trace), actual_trace(&cli_trace));
}

#[test]
fn backup_parity_retains_item_rejections_completed_files_and_stop() {
    for stop in [false, true] {
        check_backup_collision_parity(false, stop);
    }
}

#[test]
fn backup_parity_preserves_an_existing_partial_and_skips_its_partition_read() {
    for stop in [false, true] {
        check_backup_collision_parity(true, stop);
    }
}

fn check_backup_collision_parity(partial: bool, stop: bool) {
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("backup");
    let disk = disk(512, &["exists", "boot", "later"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
    let existing = if partial {
        "lun0/exists.bin.partial"
    } else {
        "lun0/exists.bin"
    };
    let fixture = |needs_inspection| {
        let mut script = initialization(false);
        if needs_inspection {
            script.extend(gpt_script(&disk, 512));
        }
        script.extend(gpt_script(&disk, 512));
        script.extend(raw_read(
            &disk[(header.last_usable_lba as usize + 1) * 512..],
        ));
        script.extend([response(RAW_ACK), bytes(&[12; 1024])]);
        let mut paused = None;
        if stop {
            let (ack, pause) = pause_response(bytes(ACK));
            script.push(ack);
            paused = Some(pause);
        } else {
            script.push(response(ACK));
            script.extend(raw_read(&[34; 1024]));
        }
        let (host, trace) = scripted_host(script);
        (CoreService::new(host), trace, paused)
    };
    let seed = || {
        fs::create_dir_all(out.join("lun0")).unwrap();
        fs::write(out.join(existing), b"old file").unwrap();
    };
    seed();
    let (service, gui_trace, pause) = fixture(true);
    let mut app = app_with_loader(service, root.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.backup.output = out.clone();
    app.update_inputs(inputs);
    let initialized = connect(&mut app);
    let before = actual_trace(&gui_trace).len();
    inspect(&mut app);
    let after = actual_trace(&gui_trace).len();
    app.select_all_partitions();
    let id = app.start().unwrap();
    if let Some(pause) = pause {
        pause.arrive();
        app.stop(Slot::Device);
        pause.release();
    }
    let report = result(&mut app, Slot::Device, id);
    assert_eq!(report.status, Status::InputError);
    let rejected = report
        .items
        .iter()
        .find(|item| item.partition.as_deref() == Some("exists"))
        .unwrap();
    assert_eq!(rejected.lun, Some(0));
    assert_eq!(rejected.execution, ExecutionState::NotAttempted);
    assert_eq!(rejected.transferred_bytes.as_deref(), Some("0"));
    assert_eq!(rejected.errors[0].category, Status::InputError);
    let gpt_reads: Vec<_> = report
        .items
        .iter()
        .filter(|item| item.partition.is_none())
        .collect();
    assert!(
        gpt_reads
            .iter()
            .all(|item| item.execution == ExecutionState::Done)
    );
    assert_eq!(
        gpt_reads
            .iter()
            .map(|item| item
                .transferred_bytes
                .as_deref()
                .unwrap()
                .parse::<u64>()
                .unwrap())
            .collect::<Vec<_>>(),
        [
            512,
            header.first_usable_lba * 512,
            (header.backup_lba - header.last_usable_lba) * 512,
        ],
    );
    assert_eq!(fs::read(out.join(existing)).unwrap(), b"old file");
    assert_eq!(out.join("lun0/exists.bin").exists(), !partial);
    assert_eq!(fs::read(out.join("lun0/boot.bin")).unwrap(), [12; 1024]);
    assert_eq!(out.join("lun0/later.bin").exists(), !stop);
    let saved = root.path().join("gui-backup");
    fs::rename(&out, &saved).unwrap();
    seed();
    let (service, cli_trace, pause) = fixture(false);
    let args = device_args(
        &["backup", "--all", "--lun", "0", "--output", path(&out)],
        &app.inputs().loader,
        "emmc",
    );
    let (code, records) = cli(&args, service, true, move |relay| {
        if let Some(pause) = pause {
            std::thread::spawn(move || {
                pause.arrive();
                relay();
                pause.release();
            });
        }
    });
    assert_eq!(code, 2);
    compare(
        records.last().unwrap(),
        gui_wire(&report, Some(&initialized)),
        stop.then_some(StopReason::User),
    );
    assert_eq!(
        trace_for_operation(&gui_trace, before, after),
        actual_trace(&cli_trace)
    );
    for read in wire_commands(&cli_trace)
        .iter()
        .filter(|cmd| cmd.name == "read")
    {
        assert_eq!(read.attributes["physical_partition_number"], "0");
        assert_eq!(read.attributes["SECTOR_SIZE_IN_BYTES"], "512");
        let start = read.attributes["start_sector"].parse::<u64>().unwrap();
        let count = read.attributes["num_partition_sectors"]
            .parse::<u64>()
            .unwrap();
        assert!(
            start + count <= 64 || start >= 66,
            "read overlaps rejected partition"
        );
    }
    for file in [
        existing,
        "lun0/boot.bin",
        "lun0/.gpt/primary.bin",
        "lun0/.gpt/backup.bin",
    ] {
        assert_eq!(
            fs::read(out.join(file)).unwrap(),
            fs::read(saved.join(file)).unwrap()
        );
    }
    assert_eq!(out.join("lun0/exists.bin").exists(), !partial);
    assert_eq!(out.join("lun0/later.bin").exists(), !stop);
}
