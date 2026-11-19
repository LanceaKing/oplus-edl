#[cfg(unix)]
use oplus_edl_core::result::ExecutionState;
#[path = "support/cloud_fixture.rs"]
mod cloud_fixture;
#[path = "support/package_fixture.rs"]
mod package_fixture;
mod support;

use oplus_edl_core::{
    Connection, CoreService, HostServices, OperationRequest, Request, StopReason, SubmitError,
    model::{
        Backend, CloudInput, IdentityMode, LoaderInput, Memory, PackageInput, PartitionSelection,
        PartitionTargets, SessionConfig,
    },
    result::Status,
};
use std::path::Path;
use support::*;

#[test]
fn integrated_cloud_acquisition_uses_its_reserved_worker_and_shared_stop() {
    use oplus_edl_core::test_support::service_with_dispatch_gate;
    use std::{sync::mpsc, thread};
    let root = tempfile::tempdir().unwrap();
    let (worker_id, workers) = mpsc::channel();
    let (running_id, running) = mpsc::channel();
    let (pause, gate) = Pause::new();
    let host = HostServices::default()
        .with_backend(
            Backend::Usb,
            || panic!("device enumeration before acquisition"),
            |_, _| panic!("device opening before acquisition"),
        )
        .with_scheduler(move |work| {
            let worker = thread::Builder::new().spawn(work)?;
            worker_id.send(worker.thread().id()).unwrap();
            Ok(())
        });
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "cloud acquisition",
        Box::new(move || {
            running_id.send(thread::current().id()).unwrap();
            gate();
        }),
    );
    let mut config = configure(root.path());
    config.loader = LoaderInput::Cloud(CloudInput {
        config: root.path().join("unused-cloud.json"),
        output: root.path().join("assets"),
        base_directory: None,
        identity_mode: IdentityMode::OpenExisting,
    });
    let handle = service.submit(Request::Connect(config)).unwrap();
    pause.arrive();
    let device_worker = workers.try_recv().unwrap();
    let acquisition_worker = workers.try_recv().unwrap();
    assert_ne!(device_worker, acquisition_worker);
    assert_eq!(running.try_recv().unwrap(), acquisition_worker);
    handle.request_stop();
    pause.release();
    let result = wait_for(|| handle.try_take_result());
    assert_eq!(result.status, Status::Stopped);
    assert_eq!(result.stop_reason, Some(StopReason::User));
    assert!(result.session.generation.is_none() && result.outputs.is_empty());
}

fn configure(root: &Path) -> SessionConfig {
    let loader = root.join("loader.elf");
    std::fs::write(&loader, b"abcd").unwrap();
    SessionConfig {
        backend: Backend::Usb,
        device: Some("scripted".into()),
        memory: Memory::Emmc,
        sector_size: None,
        wait_timeout: None,
        loader: LoaderInput::Local {
            loader: Some(loader),
            signed_digest: None,
            digest_signature: None,
        },
    }
}

fn cloud_input(root: &Path, identity: &Path) -> CloudInput {
    let config = root.join("cloud.json");
    let contents = serde_json::json!({
        "swm": {"identity_path": identity, "release_directory": root.join("release")},
        "query": {"platform": "qualcomm", "device_code": "", "scheme": "DIGEST", "pk_hash_sha256": "",
                  "pk_hash_sha384": "", "msm_id": "01234567"}
    });
    std::fs::write(&config, serde_json::to_vec(&contents).unwrap()).unwrap();
    CloudInput {
        config,
        output: root.join("assets"),
        base_directory: None,
        identity_mode: IdentityMode::OpenExisting,
    }
}

fn cloud_request(root: &Path, identity: &Path) -> Request {
    Request::Run {
        connection: Connection::Offline,
        operation: OperationRequest::CloudFetch(cloud_input(root, identity)),
    }
}

fn backup_request(root: &Path, output: std::path::PathBuf, luns: &[u8]) -> Request {
    Request::Run {
        connection: Connection::Open(configure(root)),
        operation: OperationRequest::Backup {
            output,
            overwrite: true,
            selection: PartitionSelection {
                luns: luns.to_vec(),
                partitions: PartitionTargets::All,
            },
        },
    }
}

#[test]
fn opening_the_two_slots_in_either_order_excludes_a_backup_over_swm_identity() {
    check_backup_and_swm_overlap(
        |_, backup| backup.join("lun0/identity.bin"),
        "device open",
        &[],
    );
}

#[cfg(unix)]
#[test]
fn existing_lun_and_gpt_aliases_conflict_with_swm_identity_in_both_slot_orders() {
    use std::{fs, os::unix::fs::symlink};
    for (name, identity_name) in [("lun0", "identity.bin"), ("lun0/.gpt", "primary.bin")] {
        check_backup_and_swm_overlap(
            |root, backup| {
                let actual = root.join("identity-input");
                let alias = backup.join(name);
                fs::create_dir(&actual).unwrap();
                fs::create_dir_all(alias.parent().unwrap()).unwrap();
                symlink(&actual, alias).unwrap();
                actual.join(identity_name)
            },
            "backup destinations bound",
            &["configure"],
        );
    }
}

#[cfg(unix)]
#[test]
fn retargeting_backup_aliases_after_reservation_keeps_writes_in_the_reserved_directories() {
    use oplus_edl_core::test_support::{service_with_dispatch_gate, set_dispatch_gate};
    use std::{fs, io::Cursor, os::unix::fs::symlink};

    for (name, identity_name) in [("lun0", "identity.bin"), ("lun0/.gpt", "primary.bin")] {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("backup");
        let actual = root.path().join("reserved");
        let protected = root.path().join("swm");
        fs::create_dir(&actual).unwrap();
        fs::create_dir(&protected).unwrap();
        let alias = output.join(name);
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        symlink(&actual, &alias).unwrap();
        let identity = protected.join(identity_name);
        fs::write(&identity, b"retained identity").unwrap();

        let image = disk(512, &["identity"]);
        let header = gptman::GPTHeader::read_from(&mut Cursor::new(&image[512..])).unwrap();
        let secondary = &image[(header.last_usable_lba as usize + 1) * 512..];
        let mut script = initialization(false);
        script.extend(gpt_script(&image, 512));
        script.extend([response(RAW_ACK), bytes(secondary), response(ACK)]);
        script.extend([response(RAW_ACK), bytes(&[7; 1024]), response(ACK)]);
        let (host, trace) = scripted_host(script);
        let (cloud_pause, cloud_gate) = Pause::new();
        let service = service_with_dispatch_gate(
            host,
            ManualClock::default().clock(),
            "SWM inputs bound",
            Box::new(cloud_gate),
        );
        let cloud = service
            .submit(cloud_request(root.path(), &identity))
            .unwrap();
        cloud_pause.arrive();
        cloud.request_stop();
        let (backup_pause, backup_gate) = Pause::new();
        set_dispatch_gate(&service, "backup destinations bound", Box::new(backup_gate));
        let backup = service
            .submit(backup_request(root.path(), output.clone(), &[0]))
            .unwrap();
        backup_pause.arrive();
        fs::remove_file(&alias).unwrap();
        symlink(&protected, &alias).unwrap();
        backup_pause.release();
        let result = wait_for(|| backup.try_take_result());
        cloud_pause.release();
        let cloud_result = wait_for(|| cloud.try_take_result());

        assert_eq!(result.status, Status::Success, "{result:?}");
        assert_eq!(cloud_result.status, Status::Stopped);
        assert!(cloud_result.outputs.is_empty());
        assert_eq!(fs::read(&identity).unwrap(), b"retained identity");
        assert_eq!(fs::read_dir(&protected).unwrap().count(), 1);
        let (partition, gpt) = if name == "lun0" {
            (actual.join("identity.bin"), actual.join(".gpt"))
        } else {
            (output.join("lun0/identity.bin"), actual.clone())
        };
        assert_eq!(fs::read(&partition).unwrap(), [7; 1024]);
        assert_eq!(fs::read(gpt.join("backup.bin")).unwrap(), secondary);
        assert_eq!(result.outputs.len(), 3);
        for file in [partition, gpt.join("primary.bin"), gpt.join("backup.bin")] {
            let path = file.canonicalize().unwrap();
            assert!(result.outputs.iter().any(|output| output.path == path));
        }
        assert_eq!(result.items.len(), 4);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.execution == ExecutionState::Done)
        );
        assert_eq!(
            wire_commands(&trace)
                .iter()
                .filter(|cmd| cmd.name == "read")
                .count(),
            4
        );
    }
}

#[cfg(unix)]
#[test]
fn replacing_a_reserved_gpt_directory_with_an_alias_rejects_backup_before_reads() {
    use oplus_edl_core::test_support::service_with_dispatch_gate;
    use std::{fs, os::unix::fs::symlink};
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("backup");
    let reserved = output.join("lun0/.gpt");
    fs::create_dir_all(&reserved).unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("primary.bin"), b"original").unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let (pause, gate) = Pause::new();
    let service = service_with_dispatch_gate(
        host,
        ManualClock::default().clock(),
        "backup destinations bound",
        Box::new(gate),
    );
    let backup = service
        .submit(backup_request(root.path(), output, &[0]))
        .unwrap();
    pause.arrive();
    fs::remove_dir(&reserved).unwrap();
    symlink(&other, &reserved).unwrap();
    pause.release();
    let result = wait_for(|| backup.try_take_result());
    assert_eq!(result.status, Status::InputError);
    assert!(
        result.errors[0]
            .message
            .contains("backup directory binding changed")
    );
    assert!(result.outputs.is_empty());
    assert_eq!(fs::read(other.join("primary.bin")).unwrap(), b"original");
    assert_eq!(fs::read_dir(&other).unwrap().count(), 1);
    assert!(
        wire_commands(&trace)
            .iter()
            .all(|cmd| cmd.name == "configure")
    );
}

#[cfg(unix)]
#[test]
fn backup_rejects_shared_multilun_destinations_before_any_gpt_read() {
    use std::{fs, os::unix::fs::symlink};
    for (alias, existing) in [
        ("lun1", "lun0"),
        ("lun1/.gpt", "lun0/.gpt"),
        ("lun1/.gpt", "lun0"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("backup");
        let actual = output.join(existing);
        let alias = output.join(alias);
        fs::create_dir_all(&actual).unwrap();
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        symlink(&actual, alias).unwrap();
        let sentinel = actual.join("primary.bin");
        fs::write(&sentinel, b"retained").unwrap();
        let (host, trace) = scripted_host(initialization(false));
        let service = CoreService::new(host);
        let backup = service
            .submit(backup_request(root.path(), output, &[0, 1]))
            .unwrap();
        let result = wait_for(|| backup.try_take_result());
        assert_eq!(result.status, Status::InputError);
        assert!(result.errors[0].message.contains("share a directory"));
        assert!(result.items.is_empty() && result.outputs.is_empty());
        assert_eq!(fs::read(&sentinel).unwrap(), b"retained");
        assert_eq!(fs::read_dir(&actual).unwrap().count(), 1);
        let commands = wire_commands(&trace);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "configure");
    }
}

#[cfg(unix)]
#[test]
fn swm_binding_rejects_retargeted_identity_and_release_aliases_while_backup_is_reserved() {
    use oplus_edl_core::test_support::{service_with_dispatch_gate, set_dispatch_gate};
    use std::{fs, os::unix::fs::symlink};

    for release_alias in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let output = root.join("backup");
        let protected = output.join("lun0");
        let original = root.join("swm");
        fs::create_dir_all(&protected).unwrap();
        fs::create_dir(&original).unwrap();
        let alias = root.join(if release_alias {
            "release"
        } else {
            "identity-parent"
        });
        symlink(&original, &alias).unwrap();
        let identity = if release_alias {
            original.join("identity.bin")
        } else {
            alias.join("identity.bin")
        };
        fs::write(&identity, b"swm identity").unwrap();
        fs::write(protected.join("identity.bin"), b"reserved backup file").unwrap();
        let (host, trace) = scripted_host(initialization(false));
        let (backup_pause, backup_gate) = Pause::new();
        let service = service_with_dispatch_gate(
            host,
            ManualClock::default().clock(),
            "backup destinations bound",
            Box::new(backup_gate),
        );
        let backup = service.submit(backup_request(&root, output, &[0])).unwrap();
        backup_pause.arrive();
        backup.request_stop();
        let (swm_pause, swm_gate) = Pause::new();
        set_dispatch_gate(&service, "SWM inputs bound", Box::new(swm_gate));
        let swm = service.submit(cloud_request(&root, &identity)).unwrap();
        swm_pause.arrive();
        fs::remove_file(&alias).unwrap();
        symlink(&protected, &alias).unwrap();
        swm_pause.release();
        let result = wait_for(|| swm.try_take_result());
        backup_pause.release();
        assert_eq!(
            wait_for(|| backup.try_take_result()).status,
            Status::Stopped
        );
        assert_eq!(result.status, Status::InputChanged, "{result:?}");
        let changed = if release_alias { alias } else { identity };
        assert_eq!(result.errors[0].path.as_ref(), Some(&changed));
        assert!(result.outputs.is_empty());
        assert_eq!(
            fs::read(original.join("identity.bin")).unwrap(),
            b"swm identity"
        );
        assert_eq!(
            fs::read(protected.join("identity.bin")).unwrap(),
            b"reserved backup file"
        );
        assert!(!root.join("assets").exists());
        assert!(
            wire_commands(&trace)
                .iter()
                .all(|cmd| cmd.name == "configure")
        );
    }
}

#[cfg(unix)]
#[test]
fn cloud_paths_reject_retargeted_config_and_output_while_backup_is_reserved() {
    use oplus_edl_core::test_support::{service_with_dispatch_gate, set_dispatch_gate};
    use std::{fs, os::unix::fs::symlink};
    for retarget_output in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let backup_root = root.join("backup");
        let protected = backup_root.join("lun0");
        let actual = root.join("cloud-inputs");
        let alias = root.join("cloud-parent");
        fs::create_dir_all(&protected).unwrap();
        fs::create_dir(&actual).unwrap();
        symlink(&actual, &alias).unwrap();
        let mut input = cloud_input(&root, &actual.join("identity.json"));
        if retarget_output {
            input.output = alias.join("assets");
        } else {
            fs::rename(&input.config, actual.join("cloud.json")).unwrap();
            input.config = alias.join("cloud.json");
        }
        let changed = if retarget_output {
            input.output.clone()
        } else {
            input.config.clone()
        };
        fs::write(protected.join("cloud.json"), b"backup sentinel").unwrap();
        let (host, trace) = scripted_host(initialization(false));
        let (backup_pause, backup_gate) = Pause::new();
        let service = service_with_dispatch_gate(
            host,
            ManualClock::default().clock(),
            "backup destinations bound",
            Box::new(backup_gate),
        );
        let backup = service
            .submit(backup_request(&root, backup_root, &[0]))
            .unwrap();
        backup_pause.arrive();
        backup.request_stop();
        let (cloud_pause, cloud_gate) = Pause::new();
        set_dispatch_gate(&service, "cloud paths bound", Box::new(cloud_gate));
        let cloud = service
            .submit(Request::Run {
                connection: Connection::Offline,
                operation: OperationRequest::CloudFetch(input),
            })
            .unwrap();
        cloud_pause.arrive();
        fs::remove_file(&alias).unwrap();
        symlink(&protected, &alias).unwrap();
        cloud_pause.release();
        let result = wait_for(|| cloud.try_take_result());
        backup_pause.release();
        assert_eq!(
            wait_for(|| backup.try_take_result()).status,
            Status::Stopped
        );
        assert_eq!(result.status, Status::InputChanged, "{result:?}");
        assert_eq!(result.errors[0].path.as_ref(), Some(&changed));
        assert!(result.outputs.is_empty());
        assert_eq!(
            fs::read(protected.join("cloud.json")).unwrap(),
            b"backup sentinel"
        );
        assert!(!actual.join("assets").exists() && !protected.join("assets").exists());
        assert!(
            wire_commands(&trace)
                .iter()
                .all(|cmd| cmd.name == "configure")
        );
    }
}

#[cfg(unix)]
#[test]
fn identity_creation_rechecks_the_actual_path_shown_for_confirmation() {
    use oplus_edl_core::result::ResultData;
    use std::{fs, os::unix::fs::symlink};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let alias = root.join("identity-parent");
    let original = root.join("original");
    let replacement = root.join("replacement");
    let non_directory = root.join("non-directory");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&replacement).unwrap();
    fs::write(&non_directory, b"ordinary file").unwrap();
    symlink(&original, &alias).unwrap();
    let input = cloud_input(&root, &alias.join("identity.json"));
    let service = CoreService::new(HostServices::default());
    let submit = |confirmed_path| {
        service
            .submit(Request::Run {
                connection: Connection::Offline,
                operation: OperationRequest::CloudIdentity {
                    input: input.clone(),
                    confirmed_path,
                },
            })
            .unwrap()
    };
    let review = submit(None);
    let result = wait_for(|| review.try_take_result());
    assert_eq!(result.status, Status::Success);
    let Some(ResultData::Identity {
        identity_path,
        created: false,
    }) = result.data
    else {
        panic!("missing reviewed identity path");
    };
    assert_eq!(identity_path, original.join("identity.json"));
    for target in [replacement, root.join("missing-parent"), non_directory] {
        fs::remove_file(&alias).unwrap();
        symlink(&target, &alias).unwrap();
        let creation = submit(Some(identity_path.clone()));
        let result = wait_for(|| creation.try_take_result());
        assert_eq!(result.status, Status::InputChanged);
        assert_eq!(result.errors[0].path.as_ref(), Some(&identity_path));
        assert!(result.errors[0].message.contains("review it again"));
        assert!(result.outputs.is_empty());
        assert!(!identity_path.exists() && !target.join("identity.json").exists());
    }
}

fn check_backup_and_swm_overlap(
    identity_path: impl Fn(&Path, &Path) -> std::path::PathBuf,
    backup_boundary: &'static str,
    expected_commands: &[&str],
) {
    use oplus_edl_core::test_support::service_with_dispatch_gate;
    use std::fs;
    for cloud_first in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("backup");
        let identity = identity_path(root.path(), &backup);
        fs::create_dir_all(identity.parent().unwrap()).unwrap();
        fs::write(&identity, b"retained identity").unwrap();
        let cloud_job = cloud_request(root.path(), &identity);
        let backup_job = backup_request(root.path(), backup, &[0]);
        let (host, trace) = scripted_host(if expected_commands.is_empty() {
            vec![]
        } else {
            initialization(false)
        });
        let (pause, gate) = Pause::new();
        let (first, second, boundary) = if cloud_first {
            (cloud_job, backup_job, "SWM inputs bound")
        } else {
            (backup_job, cloud_job, backup_boundary)
        };
        let service = service_with_dispatch_gate(
            host,
            ManualClock::default().clock(),
            boundary,
            Box::new(gate),
        );
        let first = service.submit(first).unwrap();
        pause.arrive();
        let second = service.submit(second).unwrap();
        let rejected = wait_for(|| second.try_take_result());
        assert_eq!(rejected.status, Status::InputError);
        assert!(rejected.errors[0].message.contains("overlapping"));
        assert!(rejected.outputs.is_empty());
        first.request_stop();
        pause.release();
        let stopped = wait_for(|| first.try_take_result());
        assert_eq!(stopped.status, Status::Stopped);
        assert!(stopped.outputs.is_empty());
        assert_eq!(fs::read(&identity).unwrap(), b"retained identity");
        assert!(!identity.with_extension("bin.partial").exists());
        assert!(!root.path().join("assets").exists());
        assert_eq!(
            wire_commands(&trace)
                .iter()
                .map(|cmd| cmd.name.as_str())
                .collect::<Vec<_>>(),
            expected_commands,
        );
    }
}

#[test]
fn independent_slots_run_together_and_reject_overlapping_paths_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("firmware");
    std::fs::create_dir(&package).unwrap();
    package_fixture::package_with_programs(&package);
    let shared = root.path().join("shared");
    let independent = root.path().join("independent");
    let mut script = initialization(false);
    let (ack, pause) = pause_response(bytes(CONFIG_ACK));
    *script.last_mut().unwrap() = ack;
    let (host, trace) = scripted_host(script);
    let service = CoreService::new(host);
    let backup = service
        .submit(Request::Run {
            connection: Connection::Open(configure(root.path())),
            operation: OperationRequest::Backup {
                output: shared.clone(),
                overwrite: false,
                selection: PartitionSelection {
                    luns: vec![0],
                    partitions: PartitionTargets::All,
                },
            },
        })
        .unwrap();
    pause.arrive();
    let prepare = |output| Request::Run {
        connection: Connection::Offline,
        operation: OperationRequest::PreparePackage {
            input: PackageInput {
                root: package.clone(),
                nv_id: "123".into(),
                config: None,
            },
            output,
        },
    };
    let overlap = service.submit(prepare(shared.clone())).unwrap();
    let rejected = wait_for(|| overlap.try_take_result());
    assert_eq!(rejected.status, Status::InputError);
    assert!(rejected.errors[0].message.contains("overlapping"));
    assert!(!shared.exists());
    let local = service.submit(prepare(independent.clone())).unwrap();
    assert!(matches!(
        service.submit(prepare(root.path().join("busy"))),
        Err(SubmitError::Busy)
    ));
    assert_eq!(wait_for(|| local.try_take_result()).status, Status::Success);
    assert!(independent.join("selection.json").exists());
    assert!(backup.try_take_result().is_none());
    backup.request_stop();
    pause.release();
    let stopped = wait_for(|| backup.try_take_result());
    assert_eq!(stopped.stop_reason, Some(StopReason::User));
    assert!(!shared.exists());
    assert_eq!(wire_commands(&trace).len(), 1);
}

#[test]
fn loopback_acquisition_uses_the_real_worker_and_never_publishes_a_bad_ticket() {
    let root = tempfile::tempdir().unwrap();
    let json = root.path().join("config.json");
    let output = root.path().join("assets");
    cloud_fixture::write_config(&json).unwrap();
    let fixture = cloud_fixture::CloudFixture::start().unwrap();
    let service =
        oplus_edl_core::test_support::loopback_service(HostServices::default(), fixture.address)
            .unwrap();
    let handle = service
        .submit(Request::Run {
            connection: Connection::Offline,
            operation: OperationRequest::CloudFetch(CloudInput {
                config: json,
                output: output.clone(),
                base_directory: None,
                identity_mode: IdentityMode::OpenExisting,
            }),
        })
        .unwrap();
    let result = wait_for(|| handle.try_take_result());
    assert_eq!(fixture.finish().unwrap(), ["enroll", "list", "ticket"]);
    assert_eq!(result.status, Status::InputError);
    assert!(
        result.errors[0]
            .message
            .contains("cloud ticket HMAC mismatch")
    );
    assert!(result.session.generation.is_none());
    assert!(result.outputs.is_empty());
    assert!(!output.exists());
}

#[test]
fn loopback_rejects_non_loopback_and_swm_without_network_or_identity_work() {
    let unsuitable = "192.0.2.1:9000".parse().unwrap();
    assert!(
        oplus_edl_core::test_support::loopback_service(HostServices::default(), unsuitable)
            .is_err()
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let root = tempfile::tempdir().unwrap();
    let json = root.path().join("config.json");
    let output = root.path().join("assets");
    let identity = root.path().join("identity.json");
    std::fs::write(
        &json,
        serde_json::to_vec(&serde_json::json!({
            "swm":{"identity_path":identity,"release_directory":root.path()},
            "query":{"platform":"qualcomm","device_code":"","scheme":"DIGEST","pk_hash_sha256":"",
                "pk_hash_sha384":"","msm_id":"002270E1"}
        }))
        .unwrap(),
    )
    .unwrap();
    let service = oplus_edl_core::test_support::loopback_service(
        HostServices::default(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    let handle = service
        .submit(Request::Run {
            connection: Connection::Offline,
            operation: OperationRequest::CloudFetch(CloudInput {
                config: json,
                output: output.clone(),
                base_directory: None,
                identity_mode: IdentityMode::CreateNew,
            }),
        })
        .unwrap();
    let result = wait_for(|| handle.try_take_result());
    assert_eq!(result.status, Status::InputError);
    assert!(
        result.errors[0]
            .message
            .contains("SWM configuration is unavailable in the loopback environment")
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    assert!(!identity.exists());
    assert!(!output.exists());
}
