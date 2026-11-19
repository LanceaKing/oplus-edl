use oplus_edl_core::result::{VerificationState, WriteState};
mod support;

use oplus_edl_core::{
    CoreService, HostServices, StopReason, SubmitError,
    model::{Backend, Decision, Verify},
    result::{ResultData, SessionDisposition, Status},
    service::Slot,
};
use oplus_edl_desktop::app::{ActionError, DesktopApp, Mode};
use std::{
    fs,
    sync::{Arc, Mutex},
};
use support::{app_with_loader, connect, inspect, protocol::*, result, summary};

fn xml_program(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("boot.xml");
    fs::write(root.join("boot.bin"), [9; 512]).unwrap();
    fs::write(&path, r#"<data><program filename="boot.bin" label="boot" SECTOR_SIZE_IN_BYTES="512" physical_partition_number="0" start_sector="64" num_partition_sectors="1"/></data>"#).unwrap();
    path
}

#[test]
fn refreshing_a_snapshot_remaps_only_the_same_partition_and_geometry() {
    let root = tempfile::tempdir().unwrap();
    let mut script = initialization(false);
    script.extend(gpt_script(&disk(512, &["boot", "system"]), 512));
    script.extend(gpt_script(&disk(512, &["boot", "vendor", "system"]), 512));
    let (host, _) = scripted_host(script);
    let mut app = app_with_loader(CoreService::new(host), root.path(), 512, false);
    connect(&mut app);
    inspect(&mut app);
    let old = app.snapshot().unwrap().clone();
    for partition in &old.partitions {
        assert!(app.select_partition(partition, true));
    }
    inspect(&mut app);
    let refreshed = app.snapshot().unwrap();
    assert_ne!(refreshed.revision, old.revision);
    assert_eq!(refreshed.generation, old.generation);
    assert_eq!(app.selected().len(), 1);
    assert!(app.selected().contains(&refreshed.partitions[0]));
    assert!(!app.select_partition(&old.partitions[0], true));
}

#[test]
fn discovery_routes_only_the_selected_provider_and_never_opens() {
    for (explicit, selected) in [
        (None, Backend::Usb),
        (Some(Backend::Usb), Backend::Usb),
        (Some(Backend::Serial), Backend::Serial),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut host = HostServices::default();
        for backend in [Backend::Usb, Backend::Serial] {
            let seen = seen.clone();
            host = host.with_backend(
                backend,
                move || {
                    seen.lock().unwrap().push(backend);
                    Ok(vec![backend.as_str().to_owned()])
                },
                |_, _| panic!("discovery tried to open a channel"),
            );
        }
        let mut app = DesktopApp::with_service(CoreService::new(host));
        let mut inputs = app.inputs().clone();
        inputs.backend = explicit;
        app.update_inputs(inputs);
        let id = app.discover().unwrap();
        let report = result(&mut app, Slot::Device, id);
        assert_eq!(report.status, Status::Success);
        assert_eq!(*seen.lock().unwrap(), [selected]);
        assert_eq!(app.candidates()[0].backend, selected);
        assert!(app.session().is_none());
    }
    let host = HostServices::default().with_backend(
        Backend::Usb,
        || panic!("unexpected fallback"),
        |_, _| panic!("unexpected open"),
    );
    let mut app = DesktopApp::with_service(CoreService::new(host));
    let mut inputs = app.inputs().clone();
    inputs.backend = Some(Backend::Serial);
    app.update_inputs(inputs);
    assert_eq!(
        app.discover(),
        Err(ActionError::Service(SubmitError::Unsupported))
    );
}

#[test]
fn stale_discovery_cannot_replace_the_current_backend_selection() {
    let (pause, callback) = Pause::new();
    let callback = Mutex::new(Some(callback));
    let host = HostServices::default()
        .with_backend(
            Backend::Usb,
            move || {
                callback.lock().unwrap().take().unwrap()();
                Ok(vec!["old-usb".into()])
            },
            |_, _| panic!("no open"),
        )
        .with_backend(
            Backend::Serial,
            || Ok(vec!["fresh-serial".into()]),
            |_, _| panic!("no open"),
        );
    let mut app = DesktopApp::with_service(CoreService::new(host));
    let id = app.discover().unwrap();
    pause.arrive();
    let mut inputs = app.inputs().clone();
    inputs.backend = Some(Backend::Serial);
    app.update_inputs(inputs);
    assert_eq!(app.discover(), Err(ActionError::Service(SubmitError::Busy)));
    pause.release();
    let report = result(&mut app, Slot::Device, id);
    assert_eq!(report.stop_reason, Some(StopReason::User));
    assert!(app.candidates().is_empty());
    let id = app.discover().unwrap();
    result(&mut app, Slot::Device, id);
    assert_eq!(app.candidates()[0].key, "fresh-serial");
}

#[test]
fn selecting_and_filtering_use_the_snapshot_key_and_empty_means_none() {
    let temp = tempfile::tempdir().unwrap();
    let disk = disk(512, &["boot", "system", "vendor"]);
    let mut script = initialization(false);
    script.extend(gpt_script(&disk, 512));
    let (host, trace) = scripted_host(script);
    let mut app = app_with_loader(CoreService::new(host), temp.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.backup.output = temp.path().join("backups");
    app.update_inputs(inputs);
    connect(&mut app);
    inspect(&mut app);
    let key = app.snapshot().unwrap().partitions[1].clone();
    assert!(app.start().is_err());
    assert!(app.select_partition(&key, true));
    app.filter_partitions("boot".into(), true);
    assert_eq!(
        app.visible_partitions()
            .map(|key| key.name.as_str())
            .collect::<Vec<_>>(),
        ["boot"]
    );
    assert_eq!(app.selected().iter().next(), Some(&key));
    app.select_all_partitions();
    assert_eq!(app.selected().len(), 3);
    app.clear_partitions();
    assert!(app.start().is_err());
    assert_eq!(
        wire_commands(&trace)
            .iter()
            .filter(|command| command.name == "read")
            .count(),
        2
    );
}

#[test]
fn a_reused_session_refuses_restore_when_the_selected_gpt_changes() {
    let temp = tempfile::tempdir().unwrap();
    let mut script = initialization(false);
    script.extend(gpt_script(&disk(512, &["boot"]), 512));
    script.extend(gpt_script(&disk(512, &["new", "boot"]), 512));
    let (host, trace) = scripted_host(script);
    let mut app = app_with_loader(CoreService::new(host), temp.path(), 512, false);
    let init = connect(&mut app);
    inspect(&mut app);
    let key = app.snapshot().unwrap().partitions[0].clone();
    let mut inputs = app.inputs().clone();
    inputs.restore.image = temp.path().join("backup.bin");
    fs::write(&inputs.restore.image, [5; 512]).unwrap();
    app.update_inputs(inputs);
    app.set_mode(Mode::Restore);
    assert!(app.select_partition(&key, true));
    let id = app.start().unwrap();
    let report = result(&mut app, Slot::Device, id);
    assert_eq!(report.status, Status::InputError);
    assert_eq!(report.errors[0].code, Some("stale_preparation"));
    assert_eq!(report.session.disposition, SessionDisposition::Reusable);
    assert_eq!(app.session(), init.session.generation);
    assert!(app.snapshot().is_none());
    assert!(!app.select_partition(&key, true));
    assert!(app.pending_write().is_none());
    let commands = wire_commands(&trace);
    assert_eq!(
        commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>(),
        ["configure", "read", "read", "read", "read"]
    );
}

#[test]
fn declined_or_invalidated_consent_cannot_be_reused() {
    let temp = tempfile::tempdir().unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let mut app = app_with_loader(CoreService::new(host), temp.path(), 512, false);
    app.set_mode(Mode::Flash);
    let mut inputs = app.inputs().clone();
    inputs.flash.programs = vec![xml_program(temp.path())];
    app.update_inputs(inputs);
    let init = connect(&mut app);

    for edit in [false, true] {
        let id = app.start().unwrap();
        let write = summary(&mut app);
        assert_eq!(write.operation_id, id);
        assert_eq!(Some(write.session_generation), init.session.generation);
        assert_eq!(app.start(), Err(ActionError::Service(SubmitError::Busy)));
        if edit {
            let mut inputs = app.inputs().clone();
            inputs.verify = Verify::None;
            app.update_inputs(inputs);
        } else {
            app.decide(write.id, Decision::Decline).unwrap();
        }
        assert!(app.decide(write.id, Decision::Approve).is_err());
        let report = result(&mut app, Slot::Device, id);
        assert_eq!(report.status, Status::InputError);
        assert_eq!(app.session(), init.session.generation);
        assert!(
            report
                .items
                .iter()
                .all(|item| item.write == WriteState::NotAttempted)
        );
    }
    assert_eq!(
        wire_commands(&trace)
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>(),
        ["configure"]
    );
}

#[test]
fn window_close_waits_for_transport_release_and_preserves_the_first_stop() {
    let temp = tempfile::tempdir().unwrap();
    let payload = [9; 512];
    let mut script = initialization(false);
    let disk = disk(512, &["boot"]);
    script.extend(gpt_script(&disk, 512));
    // Restore needs a fresh GPT; pause on the verification ACK after a successful program.
    script.extend(gpt_script(&disk, 512));
    script.extend([
        response(RAW_ACK),
        response(ACK),
        response(RAW_ACK),
        bytes(&payload),
    ]);
    let (ack, pause) = pause_response(bytes(ACK));
    script.push(ack);
    let (host, trace) = scripted_host(script);
    let mut app = app_with_loader(CoreService::new(host), temp.path(), 512, false);
    let mut inputs = app.inputs().clone();
    inputs.restore.image = temp.path().join("image.bin");
    inputs.verify = Verify::Required;
    fs::write(&inputs.restore.image, [9; 512]).unwrap();
    app.update_inputs(inputs);
    app.set_mode(Mode::Restore);
    connect(&mut app);
    inspect(&mut app);
    let target = app.snapshot().unwrap().partitions[0].clone();
    app.select_partition(&target, true);
    let id = app.start().unwrap();
    let write = summary(&mut app);
    app.decide(write.id, Decision::Approve).unwrap();
    assert!(app.decide(write.id, Decision::Approve).is_err());
    pause.arrive();
    app.stop(Slot::Device);
    app.request_window_close();
    app.tick();
    assert!(!app.close_ready());
    assert!(!trace.lock().unwrap().closed);
    pause.release();
    let report = result(&mut app, Slot::Device, id);
    assert_eq!(report.stop_reason, Some(StopReason::User));
    assert_eq!(report.session.disposition, SessionDisposition::Closed);
    assert!(
        report
            .items
            .iter()
            .any(|item| item.write == WriteState::Acknowledged
                && item.verification == VerificationState::Matched)
    );
    assert!(app.close_ready());
    assert!(trace.lock().unwrap().closed);
    assert!(app.session().is_none());
    assert!(app.snapshot().is_none());
    assert_eq!(app.result(Slot::Device).unwrap().operation_id, Some(id));
    assert_no_reset(&trace);
}

#[test]
fn cloud_identity_requires_an_explicit_base_for_relative_paths_and_rechecks_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("swm.json");
    let base = temp.path().join("chosen-base");
    let config_body = |identity: &str| {
        serde_json::json!({
            "swm": {"identity_path": identity, "release_directory": "preview"},
            "query": {"platform":"qualcomm", "device_code":"", "scheme":"DIGEST",
                "pk_hash_sha256":"", "pk_hash_sha384":"", "msm_id":"002270E1"}
        })
    };
    fs::write(
        &config,
        serde_json::to_vec(&config_body("existing.json")).unwrap(),
    )
    .unwrap();
    let mut app = DesktopApp::with_service(CoreService::new(HostServices::default()));
    let mut inputs = app.inputs().clone();
    inputs.cloud.config = config.clone();
    app.update_inputs(inputs.clone());
    let id = app.review_identity().unwrap();
    let report = result(&mut app, Slot::Offline, id);
    assert_eq!(report.status, Status::InputError);
    assert!(report.errors[0].message.contains("base directory"));
    assert!(!base.exists());

    inputs.cloud.base = Some(base.clone());
    app.update_inputs(inputs);
    let id = app.review_identity().unwrap();
    let report = result(&mut app, Slot::Offline, id);
    let approved = temp
        .path()
        .canonicalize()
        .unwrap()
        .join("chosen-base/existing.json");
    assert!(
        matches!(report.data, Some(ResultData::Identity { identity_path, created: false }) if identity_path == approved)
    );
    fs::write(
        &config,
        serde_json::to_vec(&config_body("other.json")).unwrap(),
    )
    .unwrap();
    app.confirm_identity(true).unwrap();
    let report = wait_for(|| {
        app.tick();
        app.result(Slot::Offline)
            .filter(|result| result.operation_id != Some(id))
            .cloned()
    });
    assert_eq!(report.status, Status::InputChanged);
    assert_eq!(report.errors[0].path.as_ref(), Some(&approved));
    assert!(report.errors[0].message.contains("location changed"));
    assert!(!base.exists());
}

#[test]
fn disconnect_releases_an_idle_session_and_preserves_the_result() {
    let temp = tempfile::tempdir().unwrap();
    let (host, trace) = scripted_host(initialization(false));
    let mut app = app_with_loader(CoreService::new(host), temp.path(), 512, false);
    let first = connect(&mut app);
    app.disconnect();
    wait_for(|| {
        app.tick();
        app.close_ready().then_some(())
    });
    assert!(trace.lock().unwrap().closed);
    assert!(app.session().is_none());
    assert_eq!(
        app.result(Slot::Device).unwrap().operation_id,
        first.operation_id
    );
    assert_no_reset(&trace);
}
