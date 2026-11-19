//! Scripted input for the native interaction benchmark. The production archives build binaries only.
#[path = "../../oplus-edl-core/tests/support/package_fixture.rs"]
mod package_fixture;
#[path = "../../oplus-edl-core/tests/support/mod.rs"]
mod protocol;

use eframe::{App, egui};
use gptman::{GPTHeader, GPTPartitionEntry};
use oplus_edl_core::{model::Memory, service::Slot, test_support::loopback_service};
use oplus_edl_desktop::app::{DesktopApp, FlashKind};
use std::{
    fs,
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::Duration,
};

const PARTITIONS: u32 = 10_000;
const SECTOR: usize = 512;
const DELAY: Duration = Duration::from_secs(30);

fn partition_script() -> Vec<protocol::Input> {
    let mut disk = Cursor::new(vec![0; 16_384 * SECTOR]);
    let mut header = GPTHeader::new_from(&mut disk, SECTOR as u64, [1; 16]).unwrap();
    header.number_of_partition_entries = PARTITIONS;
    header.update_from(&mut disk, SECTOR as u64).unwrap();
    let entries = (0..PARTITIONS)
        .map(|index| {
            let start = header.first_usable_lba + u64::from(index);
            GPTPartitionEntry {
                partition_type_guid: [2; 16],
                unique_partition_guid: (u128::from(index) + 1).to_le_bytes(),
                starting_lba: start,
                ending_lba: start,
                attribute_bits: 0,
                partition_name: format!("分区-{index:05}").as_str().into(),
            }
        })
        .collect::<Vec<_>>();
    header
        .write_into(&mut disk, SECTOR as u64, &entries)
        .unwrap();
    let mut script = protocol::initialization(false);
    script.extend(protocol::gpt_script(disk.get_ref(), SECTOR));
    script
}

struct Qualification {
    app: DesktopApp,
    root: tempfile::TempDir,
    inspection_submitted: bool,
    run: usize,
    error: Option<String>,
}

impl App for Qualification {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.logic(ctx, frame);
        if !self.inspection_submitted && self.app.session().is_some() {
            self.inspection_submitted = true;
            self.error = self.app.inspect().err().map(|error| error.to_string());
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("qualification-controls").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("资格测试 · 脚本设备");
                let ready = self.app.snapshot().is_some() && !self.app.busy(Slot::Offline);
                if ui
                    .add_enabled(ready, egui::Button::new("开始 30 秒离线准备"))
                    .clicked()
                {
                    self.run += 1;
                    let mut inputs = self.app.inputs().clone();
                    inputs.flash.generated =
                        self.root.path().join(format!("prepared-{}", self.run));
                    self.app.update_inputs(inputs);
                    self.error = self
                        .app
                        .prepare_package()
                        .err()
                        .map(|error| error.to_string());
                }
                if let Some(error) = &self.error {
                    ui.label(error);
                }
            });
        });
        self.app.ui(ui, frame);
    }
}

fn main() -> eframe::Result {
    let root = tempfile::tempdir().unwrap();
    let firmware = root.path().join("firmware");
    fs::create_dir(&firmware).unwrap();
    package_fixture::package_with_programs(&firmware);
    let loader = root.path().join("fixture.bin");
    fs::write(&loader, b"abcd").unwrap();

    let (host, _) = protocol::scripted_host(partition_script());
    let scheduled = AtomicUsize::new(0);
    let host = host.with_scheduler(move |task| {
        let delay = scheduled.fetch_add(1, Ordering::Relaxed) != 0;
        thread::Builder::new().spawn(move || {
            if delay {
                thread::sleep(DELAY);
            }
            task();
        })?;
        Ok(())
    });
    // Keep optional requests in this fixture on loopback as well.
    let service = loopback_service(host, "127.0.0.1:9".parse().unwrap()).unwrap();
    eframe::run_native(
        "oplus-edl-native-qualification",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Oplus EDL 交互资格测试")
                .with_inner_size([1280.0, 800.0])
                .with_min_inner_size([640.0, 480.0]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(move |context| {
            let mut app = DesktopApp::with_ui_service(&context.egui_ctx, service);
            let mut inputs = app.inputs().clone();
            inputs.device = Some("scripted".into());
            inputs.memory = Memory::Emmc;
            inputs.lun_scope = "0".into();
            inputs.loader = loader;
            inputs.flash.kind = FlashKind::GeneratePackage;
            inputs.flash.package = firmware;
            inputs.flash.nv_id = "123".into();
            app.update_inputs(inputs);
            app.connect()?;
            Ok(Box::new(Qualification {
                app,
                root,
                inspection_submitted: false,
                run: 0,
                error: None,
            }))
        }),
    )
}

#[test]
fn qualification_fixture_reaches_ten_thousand_rows_through_the_real_adapter() {
    use oplus_edl_core::{CoreService, result::Status};

    let root = tempfile::tempdir().unwrap();
    let loader = root.path().join("fixture.bin");
    fs::write(&loader, b"abcd").unwrap();
    let (host, _) = protocol::scripted_host(partition_script());
    let mut app = DesktopApp::with_service(CoreService::new(host));
    let mut inputs = app.inputs().clone();
    inputs.device = Some("scripted".into());
    inputs.memory = Memory::Emmc;
    inputs.lun_scope = "0".into();
    inputs.loader = loader;
    app.update_inputs(inputs);
    app.connect().unwrap();
    protocol::wait_for(|| {
        app.tick();
        app.session()
    });
    let id = app.inspect().unwrap();
    protocol::wait_for(|| {
        app.tick();
        app.result(Slot::Device)
            .filter(|result| result.operation_id == Some(id))
            .map(|result| {
                assert_eq!(result.status, Status::Success, "{result:?}");
            })
    });
    assert_eq!(
        app.snapshot().unwrap().partitions.len(),
        PARTITIONS as usize
    );
}
