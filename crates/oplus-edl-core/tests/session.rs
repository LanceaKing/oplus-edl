use oplus_edl_core::result::{ExecutionState, VerificationState, WriteState};
mod support;

use oplus_edl_core::{
    Connection, CoreService, Decision, HostServices, JobHandle, OperationRequest, OperationResult,
    Request, SessionConfig, StopReason,
    model::{
        Backend, FlashInput, InitializationStep, LoaderInput, Memory, PartitionSelection,
        PartitionSelector, PartitionTargets, RawSize, Scope, SessionId, Verify,
    },
    result::{OutputState, ResultData, SessionDisposition, Status},
    session::ResponseWaitExpired,
    test_support::{
        committed_partition_snapshot, service_with_dispatch_gate, service_with_lun_query_gate,
        service_with_stop_clock,
    },
};
use std::{
    fs,
    io::{Cursor, ErrorKind},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::*;

fn session_config(dir: &Path, sector: usize, digest: bool) -> SessionConfig {
    let loader = dir.join("loader.elf");
    fs::write(&loader, b"abcd").unwrap();
    let (table, signature) = if digest {
        let table = dir.join("table.bin");
        let signature = dir.join("signature.bin");
        fs::write(&table, [0xa5; 512]).unwrap();
        fs::write(&signature, [0x5a; 512]).unwrap();
        (Some(table), Some(signature))
    } else {
        (None, None)
    };
    SessionConfig {
        backend: Backend::Usb,
        device: Some("scripted".into()),
        memory: if sector == 512 {
            Memory::Emmc
        } else {
            Memory::Ufs
        },
        sector_size: Some(sector),
        loader: LoaderInput::Local {
            loader: Some(loader),
            signed_digest: table,
            digest_signature: signature,
        },
        wait_timeout: None,
    }
}

fn xml_flash(
    dir: &Path,
    sector: usize,
    verify: Verify,
    programs: usize,
    boot: Option<u8>,
    reboot: bool,
) -> OperationRequest {
    fs::write(dir.join("boot.bin"), vec![7; sector]).unwrap();
    let xml = dir.join("raw.xml");
    let commands: String = (0..programs).map(|index| {
        let start = 64 + index;
        format!(r#"<program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="{sector}" physical_partition_number="0" start_sector="{start}" num_partition_sectors="1"/>"#)
    }).collect();
    fs::write(&xml, format!("<data>{commands}</data>")).unwrap();
    OperationRequest::Flash {
        input: FlashInput::Xml {
            programs: vec![xml],
            patches: vec![],
            raw_size: RawSize::Declared,
        },
        verify,
        lun: None,
        set_bootable: boot,
        reboot,
    }
}

fn approve(handle: &JobHandle) {
    let summary = wait_for(|| handle.prepared_summary());
    assert_eq!(summary.operation_id, handle.operation_id());
    handle.decide(summary.id, Decision::Approve).unwrap();
}

fn result(handle: &JobHandle) -> OperationResult {
    wait_for(|| handle.try_take_result())
}

fn connect(service: &CoreService, config: SessionConfig) -> SessionId {
    let result = result(&service.submit(Request::Connect(config)).unwrap());
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(result.session.disposition, SessionDisposition::Reusable);
    result.session.generation.unwrap()
}

fn run_open(
    service: &CoreService,
    config: SessionConfig,
    operation: OperationRequest,
) -> JobHandle {
    service
        .submit(Request::Run {
            connection: Connection::Open(config),
            operation,
        })
        .unwrap()
}

fn run_existing(
    service: &CoreService,
    session: SessionId,
    operation: OperationRequest,
) -> JobHandle {
    service
        .submit(Request::Run {
            connection: Connection::Existing(session),
            operation,
        })
        .unwrap()
}

fn no_mutation_followup(trace: &Trace, expected: &[&str]) {
    let commands = wire_commands(trace);
    assert_eq!(
        commands
            .iter()
            .map(|cmd| cmd.name.as_str())
            .collect::<Vec<_>>(),
        expected
    );
}

fn assert_closed(result: &OperationResult, trace: &Trace) {
    assert_eq!(
        result.session.disposition,
        SessionDisposition::Closed,
        "{result:?}"
    );
    assert!(
        trace.lock().unwrap().closed,
        "terminal result preceded channel release"
    );
}

fn host_with_paused_old_channel_release(
    gate: impl FnOnce() + Send + 'static,
) -> (HostServices, Trace, Arc<AtomicUsize>) {
    let open_count = Arc::new(AtomicUsize::new(0));
    let mut transport = ScriptedIo::new(initialization(false));
    transport.on_drop = Some(Mutex::new(Box::new(gate)));
    let trace = transport.trace.clone();
    let first = Mutex::new(Some(transport));
    let opens = open_count.clone();
    let host = HostServices::default().with_backend(
        Backend::Usb,
        || Ok(vec!["scripted".into()]),
        move |_, fh_cfg| {
            opens.fetch_add(1, Ordering::SeqCst);
            let rw = first
                .lock()
                .unwrap()
                .take()
                .expect("unexpected replacement open");
            Ok(qdl::types::QdlDevice {
                rw: Box::new(rw),
                fh_cfg,
                reset_on_drop: false,
            })
        },
    );
    (host, trace, open_count)
}

#[test]
fn stop_during_previous_channel_release_prevents_replacement_open() {
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let (pause, gate) = Pause::new();
    let (host, trace, open_count) = host_with_paused_old_channel_release(gate);
    let service = CoreService::new(host);
    let old_id = connect(&service, config.clone());
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    assert!(!service.session_is_current(old_id));
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.stop_reason, Some(StopReason::User));
    assert!(result.session.generation.is_none());
    assert!(service.device_released());
    assert!(trace.lock().unwrap().closed);
    assert_eq!(open_count.load(Ordering::SeqCst), 1);
    no_mutation_followup(&trace, &["configure"]);
}

#[test]
fn discovery_revalidates_each_connection_source_before_opening() {
    for filename in ["loader.elf", "table.bin", "signature.bin"] {
        let temp = tempfile::tempdir().unwrap();
        let mut config = session_config(temp.path(), 512, true);
        config.wait_timeout = Some(std::time::Duration::from_secs(10));
        let source = temp.path().join(filename);
        let (pause, gate) = Pause::new();
        let discovery = Mutex::new(Some(gate));
        let host = HostServices::default().with_backend(
            Backend::Usb,
            move || {
                let gate = discovery.lock().unwrap().take().unwrap();
                gate();
                Ok(vec!["scripted".into()])
            },
            |_, _| panic!("changed connection materials must block opening"),
        );
        let service = CoreService::new(host);
        let handle = service.submit(Request::Connect(config)).unwrap();
        pause.arrive();
        fs::rename(&source, temp.path().join("original")).unwrap();
        fs::write(&source, b"different source during discovery").unwrap();
        pause.release();
        let report = result(&handle);
        assert_eq!(report.status, Status::InputChanged);
        assert_eq!(report.errors[0].path.as_ref(), Some(&source));
        assert!(report.session.generation.is_none());
        assert!(report.session.completed_initialization_steps.is_empty());
    }
}

#[test]
fn source_changed_during_old_channel_release_blocks_replacement_open() {
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let (pause, gate) = Pause::new();
    let (host, trace, open_count) = host_with_paused_old_channel_release(gate);
    let service = CoreService::new(host);
    let old = connect(&service, config.clone());
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    let source = temp.path().join("loader.elf");
    fs::write(&source, b"changed while the old channel releases").unwrap();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.errors[0].path.as_ref(), Some(&source));
    assert!(report.session.generation.is_none());
    assert!(!service.session_is_current(old));
    assert!(service.device_released());
    assert!(trace.lock().unwrap().closed);
    assert_eq!(open_count.load(Ordering::SeqCst), 1);
    no_mutation_followup(&trace, &["configure"]);
}

fn host_with_paused_backend_open(gate: impl FnOnce() + Send + 'static) -> (HostServices, Trace) {
    let transport = ScriptedIo::new(initialization(false));
    let trace = transport.trace.clone();
    let ready = Mutex::new(Some((gate, transport)));
    let host = HostServices::default().with_backend(
        Backend::Usb,
        || Ok(vec!["scripted".into()]),
        move |_, fh_cfg| {
            let (gate, rw) = ready.lock().unwrap().take().unwrap();
            gate();
            Ok(qdl::types::QdlDevice {
                rw: Box::new(rw),
                fh_cfg,
                reset_on_drop: false,
            })
        },
    );
    (host, trace)
}

#[test]
fn source_changed_inside_backend_open_releases_the_channel_before_upload() {
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let (pause, gate) = Pause::new();
    let (host, trace) = host_with_paused_backend_open(gate);
    let service = CoreService::new(host);
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    let source = temp.path().join("loader.elf");
    fs::write(&source, b"changed while the new channel opens").unwrap();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.errors[0].path.as_ref(), Some(&source));
    assert_eq!(report.session.reason, Some("initialization_source_changed"));
    assert!(report.session.completed_initialization_steps.is_empty());
    assert_closed(&report, &trace);
    assert!(trace.lock().unwrap().writes.is_empty());
}

#[test]
fn stop_during_backend_open_closes_the_returned_channel_before_initialization() {
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let (pause, gate) = Pause::new();
    let (host, trace) = host_with_paused_backend_open(gate);
    let service = CoreService::new(host);
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    handle.request_stop();
    assert!(handle.try_take_result().is_none());
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::Stopped);
    assert_eq!(report.stop_reason, Some(StopReason::User));
    assert_eq!(report.session.disposition, SessionDisposition::Closed);
    assert!(report.session.completed_initialization_steps.is_empty());
    assert!(service.device_released());
    let trace = trace.lock().unwrap();
    assert!(trace.closed);
    assert!(trace.writes.is_empty());
}

#[test]
fn stop_before_connect_commit_closes_the_new_session_and_keeps_completed_steps() {
    let temp = tempfile::tempdir().unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "terminal commit",
        Box::new(gate),
    );
    let handle = service
        .submit(Request::Connect(session_config(temp.path(), 512, false)))
        .unwrap();
    pause.arrive();
    let generation = handle.status().unwrap().session_generation.unwrap();
    assert!(!service.session_is_current(generation));
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.stop_reason, Some(StopReason::User));
    assert_eq!(result.session.reason, Some("initialization_stopped"));
    assert_eq!(
        result.session.completed_initialization_steps,
        [
            InitializationStep::SaharaUpload,
            InitializationStep::FirehoseWelcome,
            InitializationStep::ConfigureAck
        ]
    );
    assert_closed(&result, &trace);
    assert!(service.device_released());
    assert!(!service.session_is_current(generation));
    no_mutation_followup(&trace, &["configure"]);
}

#[test]
fn package_stop_after_sparse_prefix_or_before_commit_retains_owned_files_without_ready_data() {
    use oplus_edl_core::model::PackageInput;
    for boundary in ["sparse partial written", "terminal commit"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let firmware = root.join("firmware");
        let output = root.join("output");
        fs::create_dir(&firmware).unwrap();
        packages::package_with_programs(&firmware);
        let (pause, gate) = Pause::new();
        let service = service_with_dispatch_gate(
            HostServices::default(),
            ManualClock::default().clock(),
            boundary,
            Box::new(gate),
        );
        let handle = service
            .submit(Request::Run {
                connection: Connection::Offline,
                operation: OperationRequest::PreparePackage {
                    input: PackageInput {
                        root: firmware,
                        nv_id: "123".into(),
                        config: None,
                    },
                    output: output.clone(),
                },
            })
            .unwrap();
        pause.arrive();
        handle.request_stop();
        pause.release();
        let report = result(&handle);
        assert_eq!(report.status, Status::Stopped);
        assert_eq!(report.stop_reason, Some(StopReason::User));
        assert!(report.data.is_none());
        let outputs: std::collections::BTreeMap<_, _> = report
            .outputs
            .iter()
            .map(|file| {
                assert!(file.path.is_file());
                assert_eq!(file.path.parent(), Some(output.as_path()));
                (file.path.file_name().unwrap().to_str().unwrap(), file.state)
            })
            .collect();
        for lun in 0..5 {
            let name = format!("rawprogram{lun}.xml");
            assert_eq!(outputs[name.as_str()], OutputState::Complete);
        }
        if boundary == "sparse partial written" {
            assert_eq!(outputs.len(), 6);
            assert_eq!(outputs["super.img.partial"], OutputState::Partial);
            assert!(
                fs::metadata(output.join("super.img.partial"))
                    .unwrap()
                    .len()
                    > 28
            );
            assert!(!output.join("super.img").exists() && !output.join("selection.json").exists());
        } else {
            assert_eq!(outputs.len(), 7);
            assert_eq!(outputs["super.img"], OutputState::Complete);
            assert_eq!(outputs["selection.json"], OutputState::Complete);
        }
    }
}

#[test]
fn package_stop_after_a_raw_source_avoids_later_package_inputs_and_publication() {
    use oplus_edl_core::model::PackageInput;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let firmware = root.join("firmware");
    let output = root.join("output");
    fs::create_dir(&firmware).unwrap();
    packages::package_with_programs(&firmware);
    fs::remove_file(firmware.join("IMAGES/patch0.xml")).unwrap();
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        HostServices::default(),
        ManualClock::default().clock(),
        "package source selected",
        Box::new(gate),
    );
    let handle = service
        .submit(Request::Run {
            connection: Connection::Offline,
            operation: OperationRequest::PreparePackage {
                input: PackageInput {
                    root: firmware,
                    nv_id: "123".into(),
                    config: None,
                },
                output: output.clone(),
            },
        })
        .unwrap();
    pause.arrive();
    handle.request_stop();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::Stopped, "{report:?}");
    assert_eq!(report.stop_reason, Some(StopReason::User));
    assert!(report.errors.is_empty() && report.outputs.is_empty() && report.data.is_none());
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn replacing_the_bound_loader_with_a_fifo_cannot_reopen_it_or_access_devices() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let loader = temp.path().join("loader.elf");
    let host = HostServices::default().with_backend(
        Backend::Usb,
        || panic!("source change must stop device enumeration"),
        |_, _| panic!("source change must stop device opening"),
    );
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "loader source bound",
        Box::new(gate),
    );
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    fs::rename(&loader, loader.with_file_name("old-loader.elf")).unwrap();
    let name = CString::new(loader.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a valid C string confined to this test's temporary directory.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.errors[0].path.as_ref(), Some(&loader));
    assert!(report.session.generation.is_none());
}

#[test]
fn prepared_xml_slots_match_the_wire_for_equal_ranges() {
    let temp = tempfile::tempdir().unwrap();
    let config = session_config(temp.path(), 512, false);
    let operation = xml_flash(temp.path(), 512, Verify::None, 2, None, false);
    let xml: String = [1, 2].into_iter().map(|slot| {
        format!(r#"<program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1" slot="{slot}"/>"#)
    }).collect();
    fs::write(temp.path().join("raw.xml"), format!("<data>{xml}</data>")).unwrap();
    let mut script = initialization(false);
    script.extend([
        response(RAW_ACK),
        response(ACK),
        response(RAW_ACK),
        response(ACK),
    ]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = run_open(&service, config, operation);
    let summary = wait_for(|| handle.prepared_summary());
    assert_eq!(summary.items.len(), 2);
    assert_eq!(summary.items[0].lun, summary.items[1].lun);
    assert_eq!(summary.items[0].start_byte, summary.items[1].start_byte);
    assert_eq!(
        summary.items[0].transfer_bytes,
        summary.items[1].transfer_bytes
    );
    assert_eq!(summary.items[0].source, summary.items[1].source);
    assert_eq!(summary.items[0].slot, Some(1));
    assert_eq!(summary.items[1].slot, Some(2));
    handle.decide(summary.id, Decision::Approve).unwrap();
    let result = result(&handle);
    assert_eq!(result.status, Status::Success, "{result:?}");
    let commands = wire_commands(&trace);
    assert_eq!(commands[1].attributes["slot"], "1");
    assert_eq!(commands[2].attributes["slot"], "2");
    assert_eq!(result.items[0].slot, Some(1));
    assert_eq!(result.items[1].slot, Some(2));
}

#[test]
fn typed_decline_preserves_the_prepared_items_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let operation = xml_flash(temp.path(), 512, Verify::BestEffort, 2, Some(0), true);
    let (host, trace) = scripted_host(initialization(false));
    let service = CoreService::new(host);
    let handle = run_open(&service, session_config(temp.path(), 512, false), operation);
    let summary = wait_for(|| handle.prepared_summary());
    assert_eq!(summary.items[0].start_byte.as_deref(), Some("32768"));
    handle.decide(summary.id, Decision::Decline).unwrap();
    let result = result(&handle);
    assert_eq!(result.status, Status::InputError);
    assert_eq!(result.items.len(), summary.items.len());
    assert!(
        result
            .items
            .iter()
            .all(|item| item.execution == ExecutionState::NotAttempted
                && item.write == WriteState::NotAttempted)
    );
    no_mutation_followup(&trace, &["configure"]);
}

#[test]
fn sparse_xml_preparation_observes_stop_before_consent_or_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let operation = xml_flash(temp.path(), 512, Verify::None, 1, None, false);
    let xml = temp.path().join("raw.xml");
    let text = fs::read_to_string(&xml)
        .unwrap()
        .replace("<program ", "<program sparse=\"true\" ");
    fs::write(xml, text).unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "sparse XML parsing",
        Box::new(gate),
    );
    let handle = run_open(&service, session_config(temp.path(), 512, false), operation);
    pause.arrive();
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped, "{result:?}");
    assert_eq!(result.stop_reason, Some(StopReason::User));
    assert!(result.outputs.is_empty());
    assert!(handle.prepared_summary().is_none());
    assert!(
        wire_commands(&trace)
            .iter()
            .all(|command| command.name == "configure")
    );
}

#[test]
fn xml_stop_after_a_raw_source_avoids_opening_the_next_source() {
    let temp = tempfile::tempdir().unwrap();
    let operation = xml_flash(temp.path(), 512, Verify::None, 2, None, false);
    let xml = temp.path().join("raw.xml");
    let mut text = fs::read_to_string(&xml).unwrap();
    let next = text.rfind("boot.bin").unwrap();
    text.replace_range(next..next + "boot.bin".len(), "must-stay-unopened.bin");
    fs::write(xml, text).unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "XML operation parsed",
        Box::new(gate),
    );
    let handle = run_open(&service, session_config(temp.path(), 512, false), operation);
    pause.arrive();
    handle.request_stop();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::Stopped, "{report:?}");
    assert_eq!(report.stop_reason, Some(StopReason::User));
    assert!(report.errors.is_empty() && report.outputs.is_empty() && report.data.is_none());
    assert!(handle.prepared_summary().is_none());
    assert!(
        wire_commands(&trace)
            .iter()
            .all(|command| command.name == "configure")
    );
}

#[test]
fn unusable_terminal_program_naks_preserve_uncertainty_and_sent_bytes() {
    for (reply, status, write) in [
        (
            r#"<data><response value="NAK" rawmode="true"/></data>"#,
            Status::UnknownWrite,
            WriteState::Unknown,
        ),
        (
            r#"<data><response value="NAK" rawmode="false" AttemptRetry="1"/></data>"#,
            Status::UnknownWrite,
            WriteState::Unknown,
        ),
        (
            r#"<data><response value="NAK" AttemptRestart="1"/></data>"#,
            Status::UnknownWrite,
            WriteState::Unknown,
        ),
        (
            r#"<data><response value="NAK" rawmode="false"/></data>"#,
            Status::ExecutionFailed,
            WriteState::Failed,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), response(reply.as_bytes())]);
        let (host, trace) = scripted_host(script);
        let service = CoreService::new(host);
        let handle = run_open(
            &service,
            session_config(temp.path(), 512, false),
            xml_flash(temp.path(), 512, Verify::None, 2, None, true),
        );
        approve(&handle);
        let result = result(&handle);
        assert_eq!(result.status, status);
        assert_eq!(result.items[0].write, write);
        assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
        assert_eq!(result.items[1].write, WriteState::NotAttempted);
        assert_eq!(
            result.items[1].reason.as_deref(),
            Some("stopped after prior operation failure")
        );
        assert_closed(&result, &trace);
        no_mutation_followup(&trace, &["configure", "program"]);
    }
}

#[test]
fn partition_backup_keeps_its_ack_and_complete_bytes_when_publication_conflicts() {
    let temp = tempfile::tempdir().unwrap();
    let backup = temp.path().join("backup");
    let existing = backup.join("lun0/boot.bin");
    let conflict = existing.clone();
    let payload = vec![0xa5; 1024];
    let image = disk(512, &["boot", "other"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&image[512..])).unwrap();
    let mut script = initialization(false);
    script.extend(gpt_script(&image, 512));
    let tail = (header.last_usable_lba as usize + 1) * 512;
    script.extend([response(RAW_ACK), bytes(&image[tail..]), response(ACK)]);
    script.extend([
        response(RAW_ACK),
        bytes(&payload),
        on_consume(Access::Response, bytes(ACK), move || {
            fs::write(conflict, b"other writer").unwrap()
        }),
    ]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let session = connect(&service, session_config(temp.path(), 512, false));
    let handle = run_existing(
        &service,
        session,
        OperationRequest::Backup {
            output: backup.clone(),
            selection: PartitionSelection {
                luns: vec![0],
                partitions: PartitionTargets::All,
            },
            overwrite: false,
        },
    );
    let result = result(&handle);
    assert_eq!(result.status, Status::ExecutionFailed);
    assert_eq!(result.session.disposition, SessionDisposition::Reusable);
    let item = result
        .items
        .iter()
        .find(|item| item.partition.as_deref() == Some("boot"))
        .unwrap();
    assert_eq!(item.execution, ExecutionState::Done);
    assert_eq!(item.transferred_bytes.as_deref(), Some("1024"));
    assert!(item.errors.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("partial"));
    let partial = backup.join("lun0/boot.bin.partial").canonicalize().unwrap();
    assert!(
        result
            .outputs
            .iter()
            .any(|file| file.path == partial && file.state == OutputState::Partial)
    );
    assert_eq!(fs::read(partial).unwrap(), payload);
    assert_eq!(fs::read(existing).unwrap(), b"other writer");
    let other = result
        .items
        .iter()
        .find(|item| item.partition.as_deref() == Some("other"))
        .unwrap();
    assert_eq!(other.execution, ExecutionState::NotAttempted);
    assert!(
        other
            .reason
            .as_ref()
            .unwrap()
            .contains("publication failed")
    );
    assert!(!backup.join("lun0/other.bin").exists());
    no_mutation_followup(&trace, &["configure", "read", "read", "read", "read"]);
}

fn check_occupied_backup_partition(
    occupied_name: &str,
    gate: Option<&'static str>,
    overwrite: bool,
    occupy: fn(&Path),
    check_retained: impl Fn(&Path),
) {
    use std::io::Cursor;
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("backup");
    fs::create_dir_all(output.join("lun0")).unwrap();
    let occupied = output.join("lun0").join(occupied_name);
    let image = disk(512, &["blocked", "boot"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&image[512..])).unwrap();
    let mut script = initialization(false);
    script.extend(gpt_script(&image, 512));
    script.extend([
        response(RAW_ACK),
        bytes(&image[(header.last_usable_lba as usize + 1) * 512..]),
        response(ACK),
        response(RAW_ACK),
        bytes(&[4; 1024]),
        response(ACK),
    ]);
    let (host, trace) = scripted_host(script);
    let service = if let Some(gate) = gate {
        let occupied = occupied.clone();
        service_with_dispatch_gate(
            host,
            ManualClock::default().clock(),
            gate,
            Box::new(move || occupy(&occupied)),
        )
    } else {
        occupy(&occupied);
        CoreService::new(host)
    };
    let handle = run_open(
        &service,
        session_config(temp.path(), 512, false),
        OperationRequest::Backup {
            output: output.clone(),
            selection: PartitionSelection {
                luns: vec![0],
                partitions: PartitionTargets::All,
            },
            overwrite,
        },
    );
    let result = result(&handle);
    assert_eq!(result.status, Status::InputError, "{result:?}");
    assert_eq!(result.session.disposition, SessionDisposition::Reusable);
    assert_eq!(result.items.len(), 5);
    assert!(
        result.items[..3]
            .iter()
            .all(|item| item.execution == ExecutionState::Done)
    );
    let rejected = &result.items[3];
    assert_eq!(rejected.partition.as_deref(), Some("blocked"));
    assert_eq!(rejected.execution, ExecutionState::NotAttempted);
    assert_eq!(rejected.transferred_bytes.as_deref(), Some("0"));
    assert_eq!(rejected.errors[0].category, Status::InputError);
    assert_eq!(result.items[4].partition.as_deref(), Some("boot"));
    assert_eq!(result.items[4].execution, ExecutionState::Done);
    let owned_partial = gate.is_some() && occupied_name == "blocked.bin";
    assert_eq!(
        result
            .outputs
            .iter()
            .filter(|file| file.state == OutputState::Complete)
            .count(),
        3
    );
    assert_eq!(result.outputs.len(), 3 + usize::from(owned_partial));
    if owned_partial {
        let path = output
            .join("lun0/blocked.bin.partial")
            .canonicalize()
            .unwrap();
        assert!(
            result
                .outputs
                .iter()
                .any(|file| file.path == path && file.state == OutputState::Partial)
        );
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
    }
    check_retained(&occupied);
    assert_eq!(fs::read(output.join("lun0/boot.bin")).unwrap(), [4; 1024]);
    let reads: Vec<_> = wire_commands(&trace)
        .into_iter()
        .filter(|cmd| cmd.name == "read")
        .collect();
    assert_eq!(reads.len(), 4);
    assert_eq!(reads[3].attributes["start_sector"], "68");
    assert!(
        reads
            .iter()
            .all(|cmd| cmd.attributes["start_sector"] != "64")
    );
}

#[cfg(unix)]
#[test]
fn existing_dangling_backup_names_are_rejected_per_item_before_read() {
    use std::os::unix::fs::symlink;
    for name in ["blocked.bin", "blocked.bin.partial"] {
        check_occupied_backup_partition(
            name,
            None,
            false,
            |path| symlink("missing-target", path).unwrap(),
            |path| assert_eq!(fs::read_link(path).unwrap(), Path::new("missing-target")),
        );
    }
}

#[test]
fn exclusive_open_collision_is_a_zero_byte_item_rejection_with_overwrite() {
    check_occupied_backup_partition(
        "blocked.bin.partial",
        Some("partition output open"),
        true,
        |path| fs::write(path, b"late sentinel").unwrap(),
        |path| assert_eq!(fs::read(path).unwrap(), b"late sentinel"),
    );
}

#[test]
fn late_final_name_occupation_rejects_only_that_partition_before_read() {
    for gate in ["partition output open", "partition read"] {
        check_occupied_backup_partition(
            "blocked.bin",
            Some(gate),
            false,
            |path| fs::write(path, b"late sentinel").unwrap(),
            |path| assert_eq!(fs::read(path).unwrap(), b"late sentinel"),
        );
    }
}

#[test]
fn stop_at_mutation_dispatch_keeps_the_preceding_ack_and_sends_no_followup() {
    for command in ["patch", "set_bootable", "reboot"] {
        let temp = tempfile::tempdir().unwrap();
        let (pause, gate) = Pause::new();
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), response(ACK)]);
        let (host, trace) = scripted_host(script);
        let clock = ManualClock::default();
        let service = service_with_dispatch_gate(host, clock.clock(), command, Box::new(gate));
        let mut operation = xml_flash(
            temp.path(),
            512,
            Verify::None,
            1,
            (command == "set_bootable").then_some(0),
            command == "reboot",
        );
        if command == "patch" {
            let patch = temp.path().join("patch.xml");
            fs::write(&patch, r#"<patches><patch filename="DISK" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" byte_offset="0" size_in_bytes="4" value="1"/></patches>"#).unwrap();
            let OperationRequest::Flash {
                input: FlashInput::Xml { patches, .. },
                ..
            } = &mut operation
            else {
                unreachable!()
            };
            patches.push(patch);
        }
        let handle = run_open(&service, session_config(temp.path(), 512, false), operation);
        approve(&handle);
        pause.arrive();
        handle.request_stop();
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::Stopped, "{result:?}");
        assert_eq!(result.session.disposition, SessionDisposition::Reusable);
        assert_eq!(result.items[0].write, WriteState::Acknowledged);
        assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
        assert_eq!(result.items[1].operation, command);
        assert_eq!(result.items[1].write, WriteState::NotAttempted);
        no_mutation_followup(&trace, &["configure", "program"]);
    }
}

#[test]
fn backup_stop_at_dispatch_keeps_completed_reads_and_only_owned_partials() {
    for command in ["primary GPT read", "backup GPT read", "partition read"] {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("backup");
        let image = disk(512, &["boot"]);
        let expected_reads = match command {
            "primary GPT read" => 1,
            "backup GPT read" => 2,
            _ => 3,
        };
        let mut script = initialization(false);
        script.extend(gpt_script(&image, 512).into_iter().take(expected_reads * 3));
        if expected_reads == 3 {
            let start = image.len() - 33 * 512;
            script.extend([response(RAW_ACK), bytes(&image[start..]), response(ACK)]);
        }
        let (host, trace) = scripted_host(script);
        let clock = ManualClock::default();
        let (pause, gate) = Pause::new();
        let service = service_with_dispatch_gate(host, clock.clock(), command, Box::new(gate));
        let handle = run_open(
            &service,
            session_config(temp.path(), 512, false),
            OperationRequest::Backup {
                output: output.clone(),
                selection: selection(true),
                overwrite: false,
            },
        );
        pause.arrive();
        handle.request_stop();
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::Stopped, "{result:?}");
        assert_eq!(result.session.disposition, SessionDisposition::Reusable);
        assert_eq!(result.items.len(), expected_reads);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.execution == ExecutionState::Done)
        );
        let partial = result
            .outputs
            .iter()
            .find(|item| item.state == OutputState::Partial)
            .unwrap();
        if expected_reads == 1 {
            assert_eq!(
                partial.path,
                output
                    .join("lun0/.gpt/primary.bin.partial")
                    .canonicalize()
                    .unwrap()
            );
            assert_eq!(fs::read(&partial.path).unwrap(), image[512..1024]);
        } else {
            assert_eq!(fs::metadata(&partial.path).unwrap().len(), 0);
            assert!(output.join("lun0/.gpt/primary.bin").exists());
        }
        let mut expected = vec!["configure"];
        expected.extend(std::iter::repeat_n("read", expected_reads));
        no_mutation_followup(&trace, &expected);
    }
}

#[test]
fn raw_mode_ack_closes_configuration_or_discovery_before_another_exchange() {
    for configure in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let mut script = initialization(false);
        if configure {
            script.pop();
            let raw = std::str::from_utf8(CONFIG_ACK)
                .unwrap()
                .replace("value=\"ACK\"", "value=\"ACK\" rawmode=\"true\"");
            script.push(response(raw.as_bytes()));
        } else {
            script.push(response(b"<data><response value=\"ACK\" rawmode=\"true\" num_physical_partitions=\"1\"/></data>"));
        }
        let (host, trace) = scripted_host(script);
        let service = CoreService::new(host);
        let handle = run_open(
            &service,
            session_config(temp.path(), 512, false),
            OperationRequest::Inspect { scope: Scope::All },
        );
        let result = result(&handle);
        assert_eq!(result.status, Status::ExecutionFailed);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.message.contains("unexpected rawmode"))
        );
        assert!(result.items.is_empty() && result.data.is_none());
        assert_closed(&result, &trace);
        assert_eq!(
            result
                .session
                .completed_initialization_steps
                .contains(&InitializationStep::ConfigureAck),
            !configure
        );
        let expected: &[&str] = if configure {
            &["configure"]
        } else {
            &["configure", "getstorageinfo"]
        };
        no_mutation_followup(&trace, expected);
    }
}

#[test]
fn inspection_reports_the_actual_selected_or_discovered_gpt_coverage() {
    for scope in [Scope::Selected(vec![0]), Scope::All] {
        let temp = tempfile::tempdir().unwrap();
        let mut script = initialization(false);
        let selected = matches!(scope, Scope::Selected(_));
        if !selected {
            script.push(response(STORAGE_ACK));
        }
        script.extend(gpt_script(&disk(512, &["boot"]), 512));
        let (host, _) = scripted_host(script);
        let service = CoreService::new(host);
        let result = result(&run_open(
            &service,
            session_config(temp.path(), 512, false),
            OperationRequest::Inspect { scope },
        ));
        assert_eq!(result.status, Status::Success);
        assert!(matches!(result.data, Some(ResultData::Snapshot { .. })));
        assert_eq!(result.coverage.limited, selected);
        let scope_name = if selected {
            "explicitly selected"
        } else {
            "discovered"
        };
        assert_eq!(
            result.coverage.reason.unwrap(),
            format!("checked GPTs on {scope_name} LUNs [0]")
        );
    }
}

#[test]
fn losing_source_metadata_after_preparation_invalidates_consent_before_program() {
    let temp = tempfile::tempdir().unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let service = CoreService::new(host);
    let handle = run_open(
        &service,
        session_config(temp.path(), 512, false),
        xml_flash(temp.path(), 512, Verify::None, 1, None, false),
    );
    let prepared = wait_for(|| handle.prepared_summary());
    let source = temp.path().join("boot.bin");
    fs::rename(&source, source.with_file_name("moved-boot.bin")).unwrap();
    assert_eq!(
        fs::metadata(&source).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    handle.decide(prepared.id, Decision::Approve).unwrap();
    let result = result(&handle);
    assert_eq!(result.status, Status::InputChanged);
    assert!(result.errors.iter().any(|error| {
        error
            .path
            .as_ref()
            .is_some_and(|path| path.ends_with("boot.bin"))
    }));
    no_mutation_followup(&trace, &["configure"]);
}

#[test]
fn program_response_expiry_tracks_the_actual_mutation_and_accepted_bytes() {
    for terminal in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let config = session_config(temp.path(), 512, false);
        let operation = xml_flash(temp.path(), 512, Verify::None, 2, None, true);
        let clock = ManualClock::default();
        let (first, hold) = pause_response(Input::Error(ErrorKind::Interrupted));
        let mut script = initialization(false);
        if terminal {
            script.push(response(RAW_ACK));
        }
        script.extend([
            first,
            response(LOG),
            response(b"<data><lo"),
            on_refill(Access::Response, bytes(b"g value=\"late\"/></data>"), {
                let clock = clock.clone();
                move || clock.set(10)
            }),
        ]);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let handle = run_open(&service, config, operation);
        approve(&handle);
        hold.arrive();
        handle.request_stop();
        hold.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::UnknownWrite);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        assert_eq!(result.items[0].errors[0].cause, Some(ResponseWaitExpired));
        assert_eq!(
            result.items[0].transferred_bytes.as_deref(),
            Some(if terminal { "512" } else { "0" })
        );
        assert_eq!(result.items[1].execution, ExecutionState::NotAttempted);
        assert_closed(&result, &trace);
        no_mutation_followup(&trace, &["configure", "program"]);
    }
}

#[test]
fn timely_program_ack_preserves_the_verification_policy_and_skips_later_work() {
    for (verify, count, status, state) in [
        (
            Verify::BestEffort,
            1,
            Status::VerificationIncomplete,
            VerificationState::Incomplete,
        ),
        (
            Verify::Required,
            2,
            Status::VerificationIncomplete,
            VerificationState::Incomplete,
        ),
        (
            Verify::None,
            1,
            Status::Stopped,
            VerificationState::NotRequested,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let config = session_config(temp.path(), 512, false);
        let clock = ManualClock::default();
        let (ack, pause) = pause_response(bytes(ACK));
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), ack]);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let session = connect(&service, config);
        let handle = run_existing(
            &service,
            session,
            xml_flash(temp.path(), 512, verify, count, None, true),
        );
        approve(&handle);
        pause.arrive();
        handle.request_stop();
        clock.set(9);
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, status);
        assert_eq!(result.session.disposition, SessionDisposition::Reusable);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        let item = &result.items[0];
        assert_eq!(item.write, WriteState::Acknowledged);
        assert_eq!(item.transferred_bytes.as_deref(), Some("512"));
        assert_eq!(item.verification, state);
        assert_eq!(item.ranges[0].verification, state);
        assert!(
            result.items[1..]
                .iter()
                .all(|item| item.reason.as_deref() == Some("stop requested"))
        );
        no_mutation_followup(&trace, &["configure", "program"]);
    }
}

#[test]
fn acknowledged_write_invalidates_a_snapshot_on_a_reusable_session() {
    let temp = tempfile::tempdir().unwrap();
    let mut script = initialization(false);
    script.extend(gpt_script(&disk(512, &["boot"]), 512));
    script.extend([response(RAW_ACK), response(ACK)]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let session = connect(&service, session_config(temp.path(), 512, false));
    let scope = Scope::Selected(vec![0]);
    let inspection = service
        .submit(Request::Inspect(session, scope.clone()))
        .unwrap();
    assert_eq!(result(&inspection).status, Status::Success);
    assert!(committed_partition_snapshot(&service, session, &scope).is_some());

    let handle = run_existing(
        &service,
        session,
        xml_flash(temp.path(), 512, Verify::None, 1, None, false),
    );
    approve(&handle);
    let report = result(&handle);
    assert_eq!(report.status, Status::Success);
    assert_eq!(report.session.disposition, SessionDisposition::Reusable);
    assert_eq!(report.items[0].write, WriteState::Acknowledged);
    assert!(service.session_is_current(session));
    assert!(committed_partition_snapshot(&service, session, &scope).is_none());
    no_mutation_followup(&trace, &["configure", "read", "read", "program"]);
}

#[test]
fn later_close_preserves_the_first_stop_and_releases_the_session_before_the_result() {
    for timely in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let (ack, pause) = pause_response(bytes(ACK));
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), ack]);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let session = connect(&service, session_config(temp.path(), 512, false));
        let handle = run_existing(
            &service,
            session,
            xml_flash(temp.path(), 512, Verify::None, 1, None, true),
        );
        approve(&handle);
        pause.arrive();
        handle.request_stop();
        clock.set(5);
        service.request_close(StopReason::WindowClose);
        clock.set(if timely { 6 } else { 11 });
        pause.release();
        let result = result(&handle);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
        if timely {
            assert_eq!(result.status, Status::Stopped);
            assert_eq!(result.items[0].write, WriteState::Acknowledged);
        } else {
            assert_eq!(result.status, Status::UnknownWrite);
            assert_eq!(result.items[0].errors[0].cause, Some(ResponseWaitExpired));
        }
        assert_closed(&result, &trace);
        no_mutation_followup(&trace, &["configure", "program"]);
    }
}

#[test]
fn verification_response_expiry_preserves_program_ack_and_observed_difference() {
    for payload in [None, Some(7), Some(9)] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let (ack, pause) = pause_response(bytes(ACK));
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), response(ACK)]);
        if let Some(byte) = payload {
            script.extend([response(RAW_ACK), bytes(&[byte; 512])]);
        }
        script.push(ack);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let handle = run_open(
            &service,
            session_config(temp.path(), 512, false),
            xml_flash(temp.path(), 512, Verify::Required, 1, None, true),
        );
        approve(&handle);
        pause.arrive();
        handle.request_stop();
        clock.set(11);
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::VerificationIncomplete);
        let item = &result.items[0];
        assert_eq!(item.write, WriteState::Acknowledged);
        assert_eq!(item.transferred_bytes.as_deref(), Some("512"));
        assert_eq!(item.ranges[0].verification, VerificationState::Incomplete);
        assert_eq!(
            item.ranges[0].reason.as_deref() == Some("byte_difference_observed"),
            payload == Some(9)
        );
        assert_eq!(item.errors[0].cause, Some(ResponseWaitExpired));
        assert_closed(&result, &trace);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        no_mutation_followup(&trace, &["configure", "program", "read"]);
    }
}

#[test]
fn preview_expiry_records_raw_read_counts_after_any_completed_boot_lun_query() {
    for terminal in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let (ack, pause) = pause_response(bytes(ACK));
        let mut script = initialization(true);
        if terminal {
            script.extend([
                response(b"<data><log value=\"INFO: UFS Boot LUN ID: = 0x01\"/></data>"),
                response(ACK),
                response(RAW_ACK),
                bytes(&[0; 5 * 4096]),
            ]);
        }
        script.push(ack);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let handle = run_open(
            &service,
            session_config(temp.path(), 4096, true),
            xml_flash(
                temp.path(),
                4096,
                Verify::None,
                1,
                terminal.then_some(0),
                false,
            ),
        );
        pause.arrive();
        handle.request_stop();
        clock.set(11);
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::ExecutionFailed, "{result:?}");
        assert!(result.errors.is_empty());
        assert_eq!(result.stop_reason, Some(StopReason::User));
        if terminal {
            let query = &result.items[0];
            assert_eq!(query.operation, "getstorageinfo");
            assert_eq!(query.execution, ExecutionState::Done);
            assert_eq!(query.transferred_bytes, None);
        }
        let item = result.items.last().unwrap();
        assert_eq!(item.id, if terminal { 2 } else { 1 });
        assert_eq!(item.operation, "read");
        assert_eq!(item.execution, ExecutionState::Failed);
        assert_eq!(item.transfer_bytes.as_deref(), Some("20480"));
        assert_eq!(
            item.transferred_bytes.as_deref(),
            Some(if terminal { "20480" } else { "0" })
        );
        assert_eq!(item.errors[0].cause, Some(ResponseWaitExpired));
        assert_closed(&result, &trace);
        let expected = if terminal {
            vec![
                "verify",
                "sha256init",
                "configure",
                "getstorageinfo",
                "read",
            ]
        } else {
            vec!["verify", "sha256init", "configure", "read"]
        };
        no_mutation_followup(&trace, &expected);
    }
}

fn selection(explicit: bool) -> PartitionSelection {
    PartitionSelection {
        luns: vec![],
        partitions: if explicit {
            PartitionTargets::Selected(vec![PartitionSelector {
                lun: Some(0),
                name: "boot".into(),
            }])
        } else {
            PartitionTargets::All
        },
    }
}

#[test]
fn stop_after_gpt_header_ack_preserves_restore_read_and_backup_partial() {
    for backup in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let disk = disk(512, &["boot"]);
        let (ack, pause) = pause_response(bytes(ACK));
        let mut script = initialization(false);
        script.extend([response(RAW_ACK), bytes(&disk[512..1024]), ack]);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let image = temp.path().join("image.bin");
        fs::write(&image, [1; 512]).unwrap();
        let output = temp.path().join("backup");
        let operation = if backup {
            OperationRequest::Backup {
                output: output.clone(),
                selection: selection(true),
                overwrite: false,
            }
        } else {
            OperationRequest::Restore {
                from: None,
                image: Some(image),
                selection: selection(true),
                verify: Verify::None,
            }
        };
        let handle = run_open(&service, session_config(temp.path(), 512, false), operation);
        pause.arrive();
        handle.request_stop();
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::Stopped, "{result:?}");
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].execution, ExecutionState::Done);
        assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
        if backup {
            assert_eq!(result.items[0].reason.as_deref(), Some("GPT header probe"));
            let partial = output.join("lun0/.gpt/primary.bin.partial");
            assert_eq!(fs::read(&partial).unwrap(), disk[512..1024]);
            assert_eq!(result.outputs[0].path, partial.canonicalize().unwrap());
            assert_eq!(result.outputs[0].state, OutputState::Partial);
            assert!(!output.join("lun0/.gpt/primary.bin").exists());
        }
        assert_eq!(result.session.disposition, SessionDisposition::Reusable);
        no_mutation_followup(&trace, &["configure", "read"]);
    }
}

#[test]
fn query_stop_and_expiry_preserve_operation_error_and_dont_create_backup_output() {
    for expire in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let (ack, pause) = pause_response(bytes(STORAGE_ACK));
        let mut script = initialization(false);
        script.push(ack);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let id = connect(&service, session_config(temp.path(), 512, false));
        let output = temp.path().join("backup");
        let handle = run_existing(
            &service,
            id,
            OperationRequest::Backup {
                output: output.clone(),
                selection: selection(false),
                overwrite: false,
            },
        );
        pause.arrive();
        handle.request_stop();
        if expire {
            clock.set(11);
        }
        pause.release();
        let result = result(&handle);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        if expire {
            assert_eq!(result.status, Status::ExecutionFailed);
            assert!(result.items.is_empty());
            assert_eq!(result.errors[0].cause, Some(ResponseWaitExpired));
            assert_closed(&result, &trace);
        } else {
            assert_eq!(result.status, Status::Stopped);
            assert_eq!(result.items.len(), 1);
            assert_eq!(result.items[0].operation, "getstorageinfo");
            assert_eq!(result.items[0].execution, ExecutionState::Done);
            assert!(result.errors.is_empty());
        }
        assert!(!output.exists());
        no_mutation_followup(&trace, &["configure", "getstorageinfo"]);
    }
}

#[test]
fn reused_session_stops_before_implicit_discovery_for_run_and_inspect() {
    for inspect in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (gate, callback) = Pause::new();
        let (host, trace) = scripted_host(initialization(false));
        let service =
            service_with_lun_query_gate(host, ManualClock::default().clock(), Box::new(callback));
        let session = connect(&service, session_config(temp.path(), 512, false));
        let handle = if inspect {
            service
                .submit(Request::Inspect(session, Scope::All))
                .unwrap()
        } else {
            run_existing(
                &service,
                session,
                OperationRequest::Backup {
                    output: temp.path().join("backup"),
                    selection: selection(false),
                    overwrite: false,
                },
            )
        };
        gate.arrive();
        handle.request_stop();
        gate.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::Stopped);
        assert!(result.items.is_empty());
        assert!(result.data.is_none());
        assert_eq!(result.session.disposition, SessionDisposition::Reusable);
        no_mutation_followup(&trace, &["configure"]);
    }
}

#[test]
fn final_inspection_ack_retains_reads_and_the_live_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let disk = disk(512, &["boot"]);
    let mut script = initialization(false);
    script.extend(gpt_script(&disk, 512));
    let mut second = gpt_script(&disk, 512);
    let (ack, pause) = pause_response(second.pop().unwrap());
    script.extend(second);
    script.push(ack);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, ManualClock::default().clock());
    let session = connect(&service, session_config(temp.path(), 512, false));
    let scope = Scope::Selected(vec![0]);
    let first_handle = service
        .submit(Request::Inspect(session, scope.clone()))
        .unwrap();
    let committed = wait_for(|| committed_partition_snapshot(&service, session, &scope));
    first_handle.request_stop();
    let first = result(&first_handle);
    assert_eq!(first.status, Status::Success);
    assert_eq!(first.stop_reason, None);
    let Some(ResultData::Snapshot { snapshot }) = first.data else {
        panic!("missing snapshot")
    };
    assert_eq!(
        committed_partition_snapshot(&service, session, &scope),
        Some(snapshot.clone())
    );
    assert_eq!(snapshot, committed);
    let handle = service
        .submit(Request::Inspect(session, scope.clone()))
        .unwrap();
    pause.arrive();
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.items.len(), 2);
    for item in &result.items {
        assert_eq!(item.execution, ExecutionState::Done);
        assert_eq!(item.transferred_bytes, item.transfer_bytes);
    }
    assert!(result.data.is_none());
    assert_eq!(
        committed_partition_snapshot(&service, session, &scope),
        Some(snapshot)
    );
    no_mutation_followup(&trace, &["configure", "read", "read", "read", "read"]);
}

#[test]
fn first_inspection_ack_stops_before_the_metadata_read() {
    let temp = tempfile::tempdir().unwrap();
    let disk = disk(512, &["boot"]);
    let (ack, pause) = pause_response(bytes(ACK));
    let mut input = initialization(false);
    input.extend([response(RAW_ACK), bytes(&disk[512..1024]), ack]);
    let (host, trace) = scripted_host(input);
    let service = service_with_stop_clock(host, ManualClock::default().clock());
    let session = connect(&service, session_config(temp.path(), 512, false));
    let handle = service
        .submit(Request::Inspect(session, Scope::Selected(vec![0])))
        .unwrap();
    pause.arrive();
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].execution, ExecutionState::Done);
    assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
    assert!(result.data.is_none());
    no_mutation_followup(&trace, &["configure", "read"]);
}

#[test]
fn secondary_gpt_publish_collision_preserves_acked_bytes_and_reusable_session() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("backup");
    let disk = disk(512, &["boot"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&disk[512..])).unwrap();
    let first = (header.last_usable_lba + 1) as usize * 512;
    let last = (header.backup_lba + 1) as usize * 512;
    let mut script = initialization(false);
    script.extend(gpt_script(&disk, 512));
    let (ack, pause) = pause_response(bytes(ACK));
    script.extend([response(RAW_ACK), bytes(&disk[first..last]), ack]);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, ManualClock::default().clock());
    let id = connect(&service, session_config(temp.path(), 512, false));
    let handle = run_existing(
        &service,
        id,
        OperationRequest::Backup {
            output: output.clone(),
            selection: selection(true),
            overwrite: false,
        },
    );
    pause.arrive();
    let final_path = output.join("lun0/.gpt/backup.bin");
    fs::write(&final_path, b"racing destination").unwrap();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::ExecutionFailed, "{result:?}");
    assert_eq!(result.items[2].execution, ExecutionState::Done);
    assert_eq!(
        result.items[2].transferred_bytes,
        result.items[2].transfer_bytes
    );
    assert!(result.items[2].errors.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.session.disposition, SessionDisposition::Reusable);
    assert_eq!(fs::read(&final_path).unwrap(), b"racing destination");
    let partial = output
        .join("lun0/.gpt/backup.bin.partial")
        .canonicalize()
        .unwrap();
    assert_eq!(fs::read(&partial).unwrap(), disk[first..last]);
    assert!(
        result
            .outputs
            .iter()
            .any(|entry| entry.path == partial && entry.state == OutputState::Partial)
    );
    assert!(output.join("lun0/.gpt/primary.bin").is_file());
    no_mutation_followup(&trace, &["configure", "read", "read", "read"]);
}

#[test]
fn digest_welcome_expiry_covers_idle_timeout_and_the_post_log_marker_check() {
    for consume_marker in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let clock = ManualClock::default();
        let (first, pause) = pause_response(Input::Error(ErrorKind::Interrupted));
        let mut script = sahara_script();
        script.extend([first, response(b"  \n"), response(LOG)]);
        if consume_marker {
            script.push(on_consume(
                Access::Response,
                bytes(&[WELCOME_END, ACK].concat()),
                {
                    let clock = clock.clone();
                    move || clock.set(10)
                },
            ));
        } else {
            script.push(on_refill(
                Access::Response,
                Input::Error(ErrorKind::TimedOut),
                {
                    let clock = clock.clone();
                    move || clock.set(10)
                },
            ));
        }
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, clock.clock());
        let handle = service
            .submit(Request::Connect(session_config(temp.path(), 512, true)))
            .unwrap();
        pause.arrive();
        handle.request_stop();
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::ExecutionFailed, "{result:?}");
        assert_eq!(result.errors[0].cause, Some(ResponseWaitExpired));
        assert_eq!(
            result.session.completed_initialization_steps,
            [InitializationStep::SaharaUpload]
        );
        assert!(result.items.is_empty());
        assert_closed(&result, &trace);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        no_mutation_followup(&trace, &[]);
    }
}

#[test]
fn digest_stop_at_welcome_boundary_does_not_consume_the_following_ack() {
    for marker in [
        WELCOME_END,
        b"<data><log value=\"INFO: VIP is enabled, receiving the partition info of size 36864\"/></data>",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut script = sahara_script();
        let first = &marker[..11];
        let (tail, pause) = pause_response(bytes(&[&marker[11..], ACK].concat()));
        script.extend([response(first), tail]);
        let (host, trace) = scripted_host(script);
        let service = service_with_stop_clock(host, ManualClock::default().clock());
        let handle = service.submit(Request::Connect(session_config(temp.path(), 512, true))).unwrap();
        pause.arrive();
        let bytes_before_tail = trace.lock().unwrap().consumed;
        handle.request_stop();
        pause.release();
        let result = result(&handle);
        assert_eq!(result.status, Status::Stopped);
        assert_eq!(result.session.completed_initialization_steps, [InitializationStep::SaharaUpload, InitializationStep::FirehoseWelcome]);
        assert_eq!(trace.lock().unwrap().consumed - bytes_before_tail, marker.len() - 11);
        assert_closed(&result, &trace);
        no_mutation_followup(&trace, &[]);
    }
}

#[test]
fn digest_verify_is_one_exchange_even_when_stop_arrives_at_raw_ready() {
    let temp = tempfile::tempdir().unwrap();
    let (raw_ack, pause) = pause_response(bytes(RAW_ACK));
    let mut script = sahara_script();
    script.extend([response(WELCOME_END), response(ACK), raw_ack, response(ACK)]);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, ManualClock::default().clock());
    let handle = service
        .submit(Request::Connect(session_config(temp.path(), 512, true)))
        .unwrap();
    pause.arrive();
    handle.request_stop();
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(
        result.session.completed_initialization_steps,
        [
            InitializationStep::SaharaUpload,
            InitializationStep::FirehoseWelcome,
            InitializationStep::SignedDigestAck,
            InitializationStep::DigestSignatureAck
        ]
    );
    assert_closed(&result, &trace);
    assert_eq!(result.session.reason, Some("initialization_stopped"));
    no_mutation_followup(&trace, &["verify"]);
    let trace = trace.lock().unwrap();
    assert!(trace.writes.iter().any(|chunk| chunk == &[0x5a; 512]));
    assert!(trace.writes.iter().any(Vec::is_empty));
}

#[path = "support/package_fixture.rs"]
mod packages;

#[test]
fn package_preflight_keeps_an_acknowledged_gpt_read_before_later_expiry() {
    let temp = tempfile::tempdir().unwrap();
    packages::package_with_programs(temp.path());
    let clock = ManualClock::default();
    let disk = disk(4096, &["boot"]);
    let mut script = initialization(false);
    let (ack, pause) = pause_response(bytes(RAW_ACK));
    script.extend([
        response(RAW_ACK),
        bytes(&disk[4096..8192]),
        response(ACK),
        ack,
    ]);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, clock.clock());
    let operation = OperationRequest::Flash {
        input: FlashInput::Package {
            input: oplus_edl_core::model::PackageInput {
                root: temp.path().to_path_buf(),
                nv_id: "123".into(),
                config: None,
            },
            artifacts: oplus_edl_core::model::PackageArtifacts::Generate(temp.path().join("out")),
        },
        verify: Verify::None,
        lun: None,
        set_bootable: None,
        reboot: true,
    };
    let handle = run_open(
        &service,
        session_config(temp.path(), 4096, false),
        operation,
    );
    pause.arrive();
    handle.request_stop();
    clock.set(11);
    pause.release();
    let result = result(&handle);
    assert_closed(&result, &trace);
    assert_eq!(result.status, Status::ExecutionFailed);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].execution, ExecutionState::Done);
    assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("4096"));
    assert_eq!(result.items[1].execution, ExecutionState::Failed);
    assert_eq!(result.items[1].transferred_bytes.as_deref(), Some("0"));
    assert_eq!(result.items[1].errors[0].cause, Some(ResponseWaitExpired));
    no_mutation_followup(&trace, &["configure", "read", "read"]);
}

#[test]
fn boot_lun_query_expiry_is_operation_error_without_a_probe_or_a_write() {
    let temp = tempfile::tempdir().unwrap();
    let clock = ManualClock::default();
    let (ack, pause) = pause_response(bytes(ACK));
    let mut script = initialization(true);
    script.push(ack);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, clock.clock());
    let handle = run_open(
        &service,
        session_config(temp.path(), 4096, true),
        xml_flash(temp.path(), 4096, Verify::None, 1, Some(0), true),
    );
    pause.arrive();
    handle.request_stop();
    clock.set(11);
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::ExecutionFailed);
    assert_eq!(result.errors[0].cause, Some(ResponseWaitExpired));
    assert!(result.items.is_empty());
    assert_closed(&result, &trace);
    no_mutation_followup(
        &trace,
        &["verify", "sha256init", "configure", "getstorageinfo"],
    );
}

#[test]
fn transport_timeout_before_grace_retains_its_original_io_kind() {
    let temp = tempfile::tempdir().unwrap();
    let clock = ManualClock::default();
    let (timeout, pause) = pause_response(Input::Error(ErrorKind::TimedOut));
    let mut script = initialization(false);
    script.extend([response(RAW_ACK), timeout]);
    let (host, trace) = scripted_host(script);
    let service = service_with_stop_clock(host, clock.clock());
    let handle = run_open(
        &service,
        session_config(temp.path(), 512, false),
        xml_flash(temp.path(), 512, Verify::None, 1, None, true),
    );
    approve(&handle);
    pause.arrive();
    handle.request_stop();
    clock.set(9);
    pause.release();
    let result = result(&handle);
    assert_eq!(result.status, Status::UnknownWrite);
    assert_eq!(result.items[0].transferred_bytes.as_deref(), Some("512"));
    assert_eq!(result.items[0].errors[0].cause, None);
    assert_eq!(result.items[0].errors[0].io_kind, Some(ErrorKind::TimedOut));
    assert_closed(&result, &trace);
    no_mutation_followup(&trace, &["configure", "program"]);
}

#[test]
fn abandoning_a_job_during_consent_stops_the_accepted_job_without_programming() {
    use oplus_edl_core::service::Slot;

    let temp = tempfile::tempdir().unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let service = service_with_stop_clock(host, ManualClock::default().clock());
    let id = connect(&service, session_config(temp.path(), 512, false));
    let handle = run_existing(
        &service,
        id,
        xml_flash(temp.path(), 512, Verify::None, 1, None, true),
    );
    let summary = wait_for(|| handle.prepared_summary());
    assert_eq!(summary.session_generation, id);
    drop(handle);
    let result = wait_for(|| service.take_abandoned_result(Slot::Device));
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.stop_reason, Some(StopReason::ConsumerLost));
    assert_eq!(result.session.disposition, SessionDisposition::Reusable);
    assert_eq!(result.items[0].execution, ExecutionState::NotAttempted);
    no_mutation_followup(&trace, &["configure"]);
}

#[test]
fn reopened_package_requires_fresh_consent_and_rechecks_sources_after_it() {
    use oplus_edl_core::{
        model::{PackageArtifacts, PackageInput},
        result::InputValidity,
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    packages::package_with_programs(root);
    let output = root.join("prepared");
    oplus_edl_core::package::prepare(root, "123", &output, None, 4096).unwrap();
    let (host, trace) = scripted_host(package_project_script(23631));
    let service = CoreService::new(host);
    let operation = OperationRequest::Flash {
        input: FlashInput::Package {
            input: PackageInput {
                root: root.to_owned(),
                nv_id: "123".into(),
                config: None,
            },
            artifacts: PackageArtifacts::Reopen(output.clone()),
        },
        verify: Verify::None,
        lun: None,
        set_bootable: None,
        reboot: true,
    };
    let handle = run_open(&service, session_config(root, 4096, false), operation);
    let summary = wait_for(|| handle.prepared_summary());
    assert_eq!(summary.package.as_ref().unwrap().origin.project, 23631);
    assert!(handle.try_take_result().is_none());
    let commands = wire_commands(&trace);
    assert_eq!(commands.len(), 14);
    assert!(commands[1..].iter().all(|cmd| cmd.name == "read"));

    let image = root.join("IMAGES/gpt.bin").canonicalize().unwrap();
    let replacement = root.join("replacement.bin");
    fs::write(&replacement, [77; 4096]).unwrap();
    fs::rename(&image, root.join("retained-gpt.bin")).unwrap();
    fs::rename(&replacement, &image).unwrap();
    handle.decide(summary.id, Decision::Approve).unwrap();
    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.input_validity, InputValidity::Changed);
    assert_eq!(report.errors[0].path.as_deref(), Some(image.as_path()));
    assert_eq!(report.session.disposition, SessionDisposition::Reusable);
    assert!(report.items.iter().any(|item| item.operation == "program"));
    assert!(
        report
            .items
            .iter()
            .filter(|item| item.operation == "program")
            .all(|item| item.write == WriteState::NotAttempted)
    );
    assert!(report.outputs.is_empty());
    assert_eq!(wire_commands(&trace).len(), commands.len());
}

#[test]
fn changed_source_after_ack_keeps_ack_and_blocks_later_commands() {
    use oplus_edl_core::result::InputValidity;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let operation = xml_flash(root, 512, Verify::None, 2, Some(0), true);
    let (ack, pause) = pause_response(bytes(ACK));
    let mut script = initialization(false);
    script.extend([response(RAW_ACK), ack]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = run_open(&service, session_config(root, 512, false), operation);
    approve(&handle);
    pause.arrive();
    let source = root.join("boot.bin");
    fs::OpenOptions::new()
        .write(true)
        .open(&source)
        .unwrap()
        .set_len(1024)
        .unwrap();
    pause.release();

    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.input_validity, InputValidity::Changed);
    assert_eq!(report.items[0].write, WriteState::Acknowledged);
    assert_eq!(report.items[0].transferred_bytes.as_deref(), Some("512"));
    assert!(
        report.items[1..]
            .iter()
            .all(|item| item.write == WriteState::NotAttempted)
    );
    assert_eq!(report.session.disposition, SessionDisposition::Reusable);
    no_mutation_followup(&trace, &["configure", "program"]);
}

#[test]
fn changed_comparison_source_cannot_produce_a_matched_result() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let operation = xml_flash(root, 512, Verify::Required, 1, None, true);
    let (ack, pause) = pause_response(bytes(ACK));
    let mut script = initialization(false);
    script.extend([
        response(RAW_ACK),
        response(ACK),
        response(RAW_ACK),
        bytes(&[7; 512]),
        ack,
    ]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = run_open(&service, session_config(root, 512, false), operation);
    approve(&handle);
    pause.arrive();
    fs::remove_file(root.join("boot.bin")).unwrap();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.items[0].write, WriteState::Acknowledged);
    assert_eq!(report.items[0].verification, VerificationState::Incomplete);
    assert_eq!(
        report.items[0].ranges[0].reason.as_deref(),
        Some("verification source changed")
    );
    assert_eq!(report.session.disposition, SessionDisposition::Reusable);
    no_mutation_followup(&trace, &["configure", "program", "read"]);
}

#[test]
fn source_read_failure_inside_program_keeps_unknown_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let operation = xml_flash(root, 512, Verify::None, 1, None, true);
    let (initial_ack, pause) = pause_response(bytes(RAW_ACK));
    let mut script = initialization(false);
    script.push(initial_ack);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = run_open(&service, session_config(root, 512, false), operation);
    approve(&handle);
    pause.arrive();
    fs::OpenOptions::new()
        .write(true)
        .open(root.join("boot.bin"))
        .unwrap()
        .set_len(0)
        .unwrap();
    pause.release();
    let report = result(&handle);
    assert_eq!(report.status, Status::UnknownWrite);
    assert_eq!(
        report.input_validity,
        oplus_edl_core::result::InputValidity::Changed
    );
    assert_eq!(report.items[0].write, WriteState::Unknown);
    assert_eq!(report.items[0].transferred_bytes.as_deref(), Some("0"));
    assert_closed(&report, &trace);
    no_mutation_followup(&trace, &["configure", "program"]);
}
