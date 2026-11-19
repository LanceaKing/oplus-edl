use oplus_edl_core::result::WriteState;
mod support;

use oplus_edl_core::{
    CoreService, HostServices, Request, SubmitError,
    model::Backend,
    result::{ResultData, Status},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::wait_for;

#[test]
fn independent_caller_routes_discovery_to_only_the_requested_provider() {
    let usb = Arc::new(AtomicUsize::new(0));
    let serial = Arc::new(AtomicUsize::new(0));
    let mut host = HostServices::default();
    for (backend, calls) in [
        (Backend::Usb, usb.clone()),
        (Backend::Serial, serial.clone()),
    ] {
        host = host.with_backend(
            backend,
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![backend.as_str().to_owned()])
            },
            |_, _| panic!("discovery opened a transport"),
        );
    }
    let service = CoreService::new(host);
    for (backend, expected) in [(Backend::Usb, (1, 0)), (Backend::Serial, (1, 1))] {
        let handle = service.submit(Request::Discover { backend }).unwrap();
        assert!(matches!(
            service.submit(Request::Discover { backend }),
            Err(SubmitError::Busy)
        ));
        let result = wait_for(|| handle.try_take_result());
        assert_eq!(result.status, Status::Success);
        assert!(
            matches!(result.data, Some(ResultData::Candidates { candidates }) if candidates.len() == 1 && candidates[0].key == backend.as_str() && candidates[0].backend == backend)
        );
        assert!(result.session.generation.is_none());
        assert_eq!(
            (usb.load(Ordering::SeqCst), serial.load(Ordering::SeqCst)),
            expected
        );
    }
}

#[test]
fn missing_provider_and_worker_start_failure_happen_before_access() {
    let service = CoreService::new(HostServices::default());
    assert!(matches!(
        service.submit(Request::Discover {
            backend: Backend::Usb
        }),
        Err(SubmitError::Unsupported)
    ));

    let service = CoreService::new(
        HostServices::default()
            .with_backend(
                Backend::Usb,
                || panic!("worker never started"),
                |_, _| panic!("worker never started"),
            )
            .with_scheduler(|_| Err(std::io::Error::other("test spawn failure"))),
    );
    assert!(matches!(
        service.submit(Request::Discover {
            backend: Backend::Usb
        }),
        Err(SubmitError::WorkerStart)
    ));
}

fn local_session(root: &std::path::Path) -> oplus_edl_core::SessionConfig {
    use oplus_edl_core::model::{LoaderInput, Memory};
    let loader = root.join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    oplus_edl_core::SessionConfig {
        backend: Backend::Usb,
        device: Some("scripted".into()),
        memory: Memory::Emmc,
        sector_size: None,
        loader: LoaderInput::Local {
            loader: Some(loader),
            signed_digest: None,
            digest_signature: None,
        },
        wait_timeout: None,
    }
}

#[test]
fn empty_selected_backup_and_restore_are_rejected_before_worker_or_provider_access() {
    use oplus_edl_core::{
        Connection, OperationRequest,
        model::{PartitionSelection, PartitionTargets, Verify},
    };
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("backup");
    let config = local_session(root.path());
    let service = CoreService::new(
        HostServices::default()
            .with_backend(
                Backend::Usb,
                || panic!("empty selection reached enumeration"),
                |_, _| panic!("empty selection reached device opening"),
            )
            .with_scheduler(|_| panic!("empty selection started a worker")),
    );
    let selection = PartitionSelection {
        luns: vec![0],
        partitions: PartitionTargets::Selected(vec![]),
    };
    for operation in [
        OperationRequest::Backup {
            output: output.clone(),
            selection: selection.clone(),
            overwrite: false,
        },
        OperationRequest::Restore {
            from: Some(root.path().to_owned()),
            image: None,
            selection,
            verify: Verify::None,
        },
    ] {
        assert!(matches!(
            service.submit(Request::Run {
                connection: Connection::Open(config.clone()),
                operation,
            }),
            Err(SubmitError::InvalidRequest)
        ));
    }
    assert!(!output.exists());
}

#[test]
fn explicit_all_backs_up_discovered_scope_and_restores_only_available_scoped_mappings() {
    use oplus_edl_core::{
        Connection, Decision, OperationRequest,
        model::{PartitionSelection, PartitionTargets, Verify},
    };
    use std::{fs, io::Cursor};
    use support::*;

    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("backup");
    let mut script = initialization(false);
    let image = disk(512, &["boot", "system", "vendor"]);
    let header = gptman::GPTHeader::read_from(&mut Cursor::new(&image[512..])).unwrap();
    script.push(response(STORAGE_ACK));
    script.extend(gpt_script(&image, 512));
    script.extend([
        response(RAW_ACK),
        bytes(&image[(header.last_usable_lba as usize + 1) * 512..]),
        response(ACK),
    ]);
    for value in [1, 2, 3] {
        script.extend([response(RAW_ACK), bytes(&[value; 1024]), response(ACK)]);
    }
    script.extend(gpt_script(&image, 512));
    script.extend([
        response(RAW_ACK),
        response(ACK),
        response(RAW_ACK),
        response(ACK),
    ]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let connected = service
        .submit(Request::Connect(local_session(root.path())))
        .unwrap();
    let id = wait_for(|| connected.try_take_result())
        .session
        .generation
        .unwrap();

    let backup = service
        .submit(Request::Run {
            connection: Connection::Existing(id),
            operation: OperationRequest::Backup {
                output: output.clone(),
                selection: PartitionSelection {
                    luns: vec![],
                    partitions: PartitionTargets::All,
                },
                overwrite: false,
            },
        })
        .unwrap();
    let saved = wait_for(|| backup.try_take_result());
    assert_eq!(saved.status, Status::Success, "{saved:?}");
    assert_eq!(saved.target.unwrap().luns, [0]);
    for (name, byte) in [("boot", 1), ("system", 2), ("vendor", 3)] {
        assert_eq!(
            fs::read(output.join(format!("lun0/{name}.bin"))).unwrap(),
            [byte; 1024]
        );
    }

    fs::remove_file(output.join("lun0/vendor.bin")).unwrap();
    fs::create_dir(output.join("lun1")).unwrap();
    fs::write(output.join("lun1/boot.bin"), [9; 1024]).unwrap();
    let restore = service
        .submit(Request::Run {
            connection: Connection::Existing(id),
            operation: OperationRequest::Restore {
                from: Some(output),
                image: None,
                selection: PartitionSelection {
                    luns: vec![0],
                    partitions: PartitionTargets::All,
                },
                verify: Verify::None,
            },
        })
        .unwrap();
    let summary = wait_for(|| restore.prepared_summary());
    assert_eq!(
        summary
            .items
            .iter()
            .map(|item| (item.lun, item.partition.as_deref()))
            .collect::<Vec<_>>(),
        [(Some(0), Some("boot")), (Some(0), Some("system"))]
    );
    restore.decide(summary.id, Decision::Approve).unwrap();
    let restored = wait_for(|| restore.try_take_result());
    assert_eq!(restored.status, Status::Success, "{restored:?}");
    let commands = wire_commands(&trace);
    let writes: Vec<_> = commands
        .iter()
        .filter(|cmd| cmd.name == "program")
        .collect();
    assert_eq!(writes.len(), 2);
    assert!(
        writes
            .iter()
            .all(|cmd| cmd.attributes["physical_partition_number"] == "0")
    );
    assert_eq!(writes[0].attributes["start_sector"], "64");
    assert_eq!(writes[1].attributes["start_sector"], "68");
}

#[test]
fn collecting_connect_allows_reuse_but_old_generations_and_revisions_are_rejected() {
    use oplus_edl_core::{
        Connection, OperationRequest,
        model::{PartitionSelection, PartitionTargets, Scope, SessionId},
    };
    use support::*;
    let root = tempfile::tempdir().unwrap();
    let mut script = initialization(false);
    let mock_disk = disk(512, &["boot"]);
    script.extend(gpt_script(&mock_disk, 512));
    script.extend(gpt_script(&mock_disk, 512));
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = service
        .submit(Request::Connect(local_session(root.path())))
        .unwrap();
    let first = wait_for(|| handle.try_take_result());
    let session = first.session.generation.unwrap();
    drop(handle);
    assert!(service.session_is_current(session));
    assert!(matches!(
        service.submit(Request::Inspect(SessionId(session.0 + 1), Scope::All)),
        Err(SubmitError::StaleSession)
    ));

    let mut snapshots = Vec::new();
    for _ in 0..2 {
        let handle = service
            .submit(Request::Inspect(session, Scope::Selected(vec![0])))
            .unwrap();
        let result = wait_for(|| handle.try_take_result());
        assert!(result.session.completed_initialization_steps.is_empty());
        let Some(ResultData::Snapshot { snapshot }) = result.data else {
            panic!("expected snapshot")
        };
        snapshots.push(snapshot);
    }
    assert!(!service.snapshot_is_current(session, snapshots[0].revision));
    assert!(service.snapshot_is_current(session, snapshots[1].revision));
    let invalid = service.submit_for_snapshot(
        Request::Run {
            connection: Connection::Existing(session),
            operation: OperationRequest::Backup {
                output: root.path().join("backup"),
                selection: PartitionSelection {
                    luns: vec![0],
                    partitions: PartitionTargets::All,
                },
                overwrite: false,
            },
        },
        session,
        snapshots[0].revision,
    );
    assert!(matches!(invalid, Err(SubmitError::StaleSnapshot)));
    assert_eq!(
        wire_commands(&trace)
            .iter()
            .map(|cmd| cmd.name.as_str())
            .collect::<Vec<_>>(),
        ["configure", "read", "read", "read", "read"]
    );
    assert!(service.close_idle());
    assert!(service.device_released());
    assert_no_reset(&trace);
}

#[test]
fn completion_and_reusable_session_survive_a_saturated_event_ring() {
    use oplus_edl_core::model::SessionId;
    use support::*;
    let root = tempfile::tempdir().unwrap();
    let mut script = sahara_script();
    let log = format!("<data><log value=\"{}\"/></data>", "x".repeat(2300));
    for _ in 0..400 {
        script.push(response(log.as_bytes()));
    }
    script.extend([response(WELCOME_END), response(CONFIG_ACK)]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = service
        .submit(Request::Connect(local_session(root.path())))
        .unwrap();
    wait_for(|| service.session_is_current(SessionId(1)).then_some(()));
    let health = handle.status().unwrap();
    assert!(health.dropped_logs > 0);
    let mut count = 0;
    let mut bytes = 0;
    let mut redacted = 0;
    while let Some(event) = handle.poll_event() {
        assert_eq!(event.operation_id, Some(handle.operation_id()));
        assert!(event.message.len() <= 8192);
        assert!(!event.message.contains("xxxxxxxx"));
        redacted += usize::from(event.message == "Firehose: startup log received");
        count += 1;
        bytes += event.message.len();
    }
    assert!((1..=256).contains(&count));
    assert!(redacted > 0);
    assert!(bytes <= 512 * 1024);
    let result = wait_for(|| handle.try_take_result());
    assert_eq!(result.status, Status::Success);
    assert_eq!(result.session.generation, Some(SessionId(1)));
    assert!(service.close_idle());
    assert!(trace.lock().unwrap().closed);
}

#[test]
fn dropping_service_finishes_an_outstanding_ack_and_closes_on_its_worker() {
    use oplus_edl_core::{
        Connection, OperationRequest, StopReason,
        model::{Decision, FlashInput, RawSize, Verify},
        result::SessionDisposition,
    };
    use support::*;
    let root = tempfile::tempdir().unwrap();
    let xml = root.path().join("program.xml");
    std::fs::write(root.path().join("source.bin"), [9; 512]).unwrap();
    std::fs::write(&xml, r#"<data><program filename="source.bin" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
    let mut script = initialization(false);
    let (ack, pause) = pause_response(bytes(ACK));
    script.extend([response(RAW_ACK), ack]);
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let handle = service
        .submit(Request::Run {
            connection: Connection::Open(local_session(root.path())),
            operation: OperationRequest::Flash {
                input: FlashInput::Xml {
                    programs: vec![xml],
                    patches: vec![],
                    raw_size: RawSize::Declared,
                },
                verify: Verify::None,
                lun: None,
                set_bootable: None,
                reboot: true,
            },
        })
        .unwrap();
    let write = wait_for(|| handle.prepared_summary());
    assert_eq!(write.operation_id, handle.operation_id());
    assert!(matches!(
        service.submit(Request::Discover {
            backend: Backend::Usb
        }),
        Err(SubmitError::Busy)
    ));
    handle.decide(write.id, Decision::Approve).unwrap();
    assert!(handle.decide(write.id, Decision::Approve).is_err());
    pause.arrive();
    drop(service);
    assert!(!trace.lock().unwrap().closed);
    pause.release();
    let report = wait_for(|| handle.try_take_result());
    assert_eq!(report.stop_reason, Some(StopReason::ConsumerLost));
    assert_eq!(report.status, Status::Stopped);
    assert_eq!(report.session.disposition, SessionDisposition::Closed);
    assert_eq!(report.items[0].write, WriteState::Acknowledged);
    assert!(trace.lock().unwrap().closed);
    assert_no_reset(&trace);
}
