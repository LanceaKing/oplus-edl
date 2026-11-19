#![allow(dead_code)]

#[path = "../../../oplus-edl-core/tests/support/package_fixture.rs"]
pub mod package_fixture;
#[path = "../../../oplus-edl-core/tests/support/mod.rs"]
pub mod protocol;

use oplus_edl_core::{
    CoreService, OperationResult,
    model::{Memory, PreparedSummary},
    result::Status,
    service::Slot,
};
use oplus_edl_desktop::app::DesktopApp;
use protocol::wait_for;
use std::{fs, path::Path};

pub fn app_with_loader(
    service: CoreService,
    root: &Path,
    sector: usize,
    digest: bool,
) -> DesktopApp {
    let mut app = DesktopApp::with_service(service);
    let mut inputs = app.inputs().clone();
    inputs.loader = root.join("loader.elf");
    fs::write(&inputs.loader, b"abcd").unwrap();
    if digest {
        let table = root.join("table.bin");
        let signature = root.join("signature.bin");
        fs::write(&table, [0xa5; 512]).unwrap();
        fs::write(&signature, [0x5a; 512]).unwrap();
        inputs.signed_digest = Some(table);
        inputs.digest_signature = Some(signature);
    }
    inputs.device = Some("scripted".into());
    inputs.memory = if sector == 512 {
        Memory::Emmc
    } else {
        Memory::Ufs
    };
    inputs.lun_scope = "0".into();
    app.update_inputs(inputs);
    app
}

pub fn result(app: &mut DesktopApp, slot: Slot, id: u64) -> OperationResult {
    wait_for(|| {
        app.tick();
        app.result(slot)
            .filter(|result| result.operation_id == Some(id))
            .cloned()
    })
}

pub fn connect(app: &mut DesktopApp) -> OperationResult {
    let id = app.connect().unwrap();
    let result = result(app, Slot::Device, id);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert_eq!(app.session(), result.session.generation);
    result
}

pub fn inspect(app: &mut DesktopApp) -> OperationResult {
    let id = app.inspect().unwrap();
    let result = result(app, Slot::Device, id);
    assert_eq!(result.status, Status::Success, "{result:?}");
    assert!(app.snapshot().is_some());
    result
}

pub fn summary(app: &mut DesktopApp) -> PreparedSummary {
    wait_for(|| {
        app.tick();
        app.pending_write().cloned()
    })
}

pub fn raw_read(data: &[u8]) -> [protocol::Input; 3] {
    [
        protocol::response(protocol::RAW_ACK),
        protocol::bytes(data),
        protocol::response(protocol::ACK),
    ]
}
