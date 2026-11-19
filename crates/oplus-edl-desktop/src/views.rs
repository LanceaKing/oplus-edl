use eframe::egui::{
    self, Button, Color32, Frame, Label, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui,
};
use egui_extras::{Column, TableBuilder};
use oplus_edl_core::result::{ExecutionState, VerificationState, WriteState};

use oplus_edl_core::{
    model::{Backend, Memory, RawSize, Verify},
    package::Preparation,
    result::{Item, OperationResult, ResultData, Status},
    service::Slot,
};
use std::{path::PathBuf, time::Duration};

use crate::{
    app::{DesktopApp, FlashKind, Inputs, Mode, RestoreKind, SidePanel},
    dialogs::{Picker, optional_path, path_field},
};

const INK: Color32 = Color32::from_rgb(24, 38, 52);
const MUTED: Color32 = Color32::from_rgb(81, 101, 118);
const ACCENT: Color32 = Color32::from_rgb(14, 101, 112);
const WASH: Color32 = Color32::from_rgb(232, 244, 245);
const PANEL: Color32 = Color32::from_rgb(245, 248, 250);
const LINE: Color32 = Color32::from_rgb(216, 226, 234);
const WARNING: Color32 = Color32::from_rgb(147, 74, 14);

pub(crate) fn configure_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 28.0;
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = Color32::WHITE;
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.window_stroke = Stroke::new(1.0, LINE);
        style.visuals.extreme_bg_color = Color32::WHITE;
        style.visuals.faint_bg_color = PANEL;
        style.visuals.selection.bg_fill = WASH;
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(236, 242, 246);
        style.visuals.widgets.hovered.weak_bg_fill = WASH;
    });
}

pub(crate) fn primary_button(label: &str) -> Button<'_> {
    Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .stroke(Stroke::new(1.0, ACCENT))
}

pub(crate) fn workspace(app: &mut DesktopApp, ui: &mut Ui, frame: &eframe::Frame) {
    let compact = ui.available_width() < 920.0;
    egui::Panel::top("navigation")
        .resizable(false)
        .frame(panel_frame(Color32::WHITE))
        .show(ui, |ui| header(app, ui, compact));
    egui::Panel::bottom("work_progress")
        .resizable(false)
        .frame(panel_frame(PANEL))
        .show(ui, |ui| progress(app, ui));

    if !compact {
        egui::Panel::left("operation_inputs")
            .resizable(false)
            .exact_size(249.0)
            .frame(panel_frame(PANEL))
            .show(ui, |ui| inputs(app, ui, frame));
        egui::Panel::right("operation_status")
            .resizable(false)
            .exact_size(248.0)
            .frame(panel_frame(PANEL))
            .show(ui, |ui| status(app, ui));
    }

    egui::CentralPanel::default()
        .frame(panel_frame(Color32::WHITE))
        .show(ui, |ui| match app.compact_panel.filter(|_| compact) {
            Some(SidePanel::Inputs) => inputs(app, ui, frame),
            Some(SidePanel::Status) => status(app, ui),
            None => center(app, ui),
        });
}

fn panel_frame(fill: Color32) -> Frame {
    Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(14, 10))
}

fn header(app: &mut DesktopApp, ui: &mut Ui, compact: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Oplus EDL").size(24.0).strong());
        if let Some(device) = &app.connected {
            ui.label(
                RichText::new(device.target.device.as_deref().unwrap_or("已连接")).color(ACCENT),
            );
        } else if app.disconnecting {
            ui.label("正在释放设备会话");
        } else {
            ui.label(RichText::new("设备未连接").color(MUTED));
        }
    });
    ui.horizontal_wrapped(|ui| {
        for mode in Mode::ALL {
            let selected = app.mode == mode && (!compact || app.compact_panel.is_none());
            if ui
                .add_enabled(
                    app.pending_write().is_none() && !app.closing,
                    Button::new(mode.label()).selected(selected),
                )
                .clicked()
            {
                app.set_mode(mode);
            }
        }
        if compact {
            ui.separator();
            for (label, side) in [
                ("输入", SidePanel::Inputs),
                ("状态与日志", SidePanel::Status),
            ] {
                let selected = app.compact_panel == Some(side);
                if ui.add(Button::new(label).selected(selected)).clicked() {
                    app.compact_panel = if selected { None } else { Some(side) };
                }
            }
        }
    });
    if let Some(notice) = &app.notice {
        ui.label(RichText::new(notice).color(WARNING));
    }
}

fn inputs(app: &mut DesktopApp, ui: &mut Ui, frame: &eframe::Frame) {
    ScrollArea::vertical()
        .id_salt("input_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let device_idle = !app.busy(Slot::Device) && !app.disconnecting && !app.closing;
            let offline_idle = !app.busy(Slot::Offline) && !app.closing;
            let mut next = app.inputs().clone();
            ui.heading("设备与加载器");
            ui.add_enabled_ui(device_idle, |ui| {
                connection_inputs(ui, frame, &mut next, app)
            });
            app.update_inputs(next.clone());
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(device_idle, Button::new("发现设备"))
                    .clicked()
                {
                    let result = app.discover();
                    app.remember_error(result);
                }
                if ui
                    .add_enabled(device_idle && app.session().is_none(), Button::new("连接"))
                    .clicked()
                {
                    let result = app.connect();
                    app.remember_error(result);
                }
                if ui
                    .add_enabled(
                        (app.session().is_some() || app.busy(Slot::Device))
                            && !app.disconnecting
                            && !app.closing,
                        Button::new("断开"),
                    )
                    .clicked()
                {
                    app.disconnect();
                }
            });
            ui.separator();
            ui.add_enabled_ui(device_idle && !app.busy(Slot::Offline), |ui| {
                match app.mode {
                    Mode::Backup => {
                        ui.heading("备份输出");
                        path_field(
                            ui,
                            frame,
                            "备份目录",
                            &mut next.backup.output,
                            Picker::Folder,
                        );
                        ui.checkbox(&mut next.backup.overwrite, "允许替换已有正式文件");
                    }
                    Mode::Restore => restore_inputs(ui, frame, &mut next),
                    Mode::Flash => flash_inputs(ui, frame, &mut next),
                }
                if app.mode != Mode::Backup {
                    ui.separator();
                    egui::ComboBox::from_id_salt("verify")
                        .selected_text(verify_label(next.verify))
                        .show_ui(ui, |ui| {
                            for option in [Verify::BestEffort, Verify::Required, Verify::None] {
                                ui.selectable_value(&mut next.verify, option, verify_label(option));
                            }
                        });
                }
            });
            app.update_inputs(next);
            if app.mode == Mode::Flash
                && app.inputs().flash.kind == FlashKind::GeneratePackage
                && ui
                    .add_enabled(offline_idle, Button::new("仅离线准备固件包"))
                    .clicked()
            {
                let result = app.prepare_package();
                app.remember_error(result);
            }
            ui.separator();
            egui::CollapsingHeader::new("云端资产与安装身份").show(ui, |ui| {
                let mut next = app.inputs().clone();
                ui.add_enabled_ui(device_idle && offline_idle, |ui| {
                    path_field(ui, frame, "云端配置", &mut next.cloud.config, Picker::File);
                    path_field(
                        ui,
                        frame,
                        "新资产目录",
                        &mut next.cloud.output,
                        Picker::Folder,
                    );
                    optional_path(
                        ui,
                        frame,
                        "相对路径的基目录",
                        &mut next.cloud.base,
                        Picker::Folder,
                    );
                    ui.checkbox(&mut next.use_cloud, "连接时获取云端加载器");
                });
                app.update_inputs(next);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(offline_idle, Button::new("获取资产"))
                        .clicked()
                    {
                        let result = app.fetch_cloud();
                        app.remember_error(result);
                    }
                    if ui
                        .add_enabled(offline_idle, Button::new("创建安装身份…"))
                        .clicked()
                    {
                        let result = app.review_identity();
                        app.remember_error(result);
                    }
                    if device_idle
                        && matches!(app.offline_data(), Some(ResultData::Assets { .. }))
                        && ui.button("使用已获取的资产").clicked()
                    {
                        app.use_cloud_assets();
                    }
                });
            });
        });
}

fn connection_inputs(ui: &mut Ui, frame: &eframe::Frame, inputs: &mut Inputs, app: &DesktopApp) {
    let current = inputs.discovery_backend();
    let mut backend = current;
    egui::ComboBox::from_id_salt("backend")
        .selected_text(current.as_str())
        .show_ui(ui, |ui| {
            for option in [Backend::Usb, Backend::Serial] {
                ui.selectable_value(&mut backend, option, option.as_str());
            }
        });
    if backend != current {
        inputs.backend = Some(backend);
        inputs.device = None;
    }
    egui::ComboBox::from_id_salt("candidates")
        .selected_text(inputs.device.as_deref().unwrap_or("选择设备"))
        .show_ui(ui, |ui| {
            if backend == Backend::Usb {
                ui.selectable_value(&mut inputs.device, None, "自动选择唯一 USB 设备");
            }
            for candidate in app
                .candidates()
                .iter()
                .filter(|candidate| candidate.backend == backend)
            {
                ui.selectable_value(
                    &mut inputs.device,
                    Some(candidate.key.clone()),
                    &candidate.key,
                );
            }
        });
    let mut selected = inputs.device.clone().unwrap_or_default();
    if ui
        .add(TextEdit::singleline(&mut selected).hint_text("设备键或串口路径"))
        .changed()
    {
        inputs.device = (!selected.is_empty()).then_some(selected);
    }
    if !inputs.use_cloud {
        path_field(ui, frame, "加载器", &mut inputs.loader, Picker::File);
        egui::CollapsingHeader::new("加载器附加材料").show(ui, |ui| {
            optional_path(
                ui,
                frame,
                "签名摘要",
                &mut inputs.signed_digest,
                Picker::File,
            );
            optional_path(
                ui,
                frame,
                "DIGEST 签名",
                &mut inputs.digest_signature,
                Picker::File,
            );
        });
    }
    egui::CollapsingHeader::new("连接参数").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut inputs.memory, Memory::Ufs, "UFS");
            ui.selectable_value(&mut inputs.memory, Memory::Emmc, "eMMC");
        });
        egui::ComboBox::from_id_salt("sector_size")
            .selected_text(
                inputs
                    .sector_size
                    .map_or_else(|| "介质默认扇区".into(), |size| format!("{size} 字节/扇区")),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut inputs.sector_size, None, "介质默认扇区");
                for size in [512, 4096] {
                    ui.selectable_value(
                        &mut inputs.sector_size,
                        Some(size),
                        format!("{size} 字节/扇区"),
                    );
                }
            });
        let mut waits = inputs.wait_seconds.is_some();
        if ui.checkbox(&mut waits, "等待设备发现").changed() {
            inputs.wait_seconds = waits.then_some(30);
        }
        if let Some(seconds) = &mut inputs.wait_seconds {
            ui.add(
                egui::DragValue::new(seconds)
                    .range(0..=u64::MAX)
                    .suffix(" 秒"),
            );
            ui.small("0 表示等待到设备出现或任务停止");
        }
    });
}

fn restore_inputs(ui: &mut Ui, frame: &eframe::Frame, inputs: &mut Inputs) {
    ui.heading("恢复来源");
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut inputs.restore.kind, RestoreKind::Image, "单个镜像");
        ui.selectable_value(&mut inputs.restore.kind, RestoreKind::Directory, "备份目录");
    });
    match inputs.restore.kind {
        RestoreKind::Image => path_field(
            ui,
            frame,
            "镜像文件",
            &mut inputs.restore.image,
            Picker::File,
        ),
        RestoreKind::Directory => {
            path_field(
                ui,
                frame,
                "恢复目录",
                &mut inputs.restore.directory,
                Picker::Folder,
            );
            ui.checkbox(&mut inputs.restore.all, "恢复范围内现有的全部映射");
        }
    }
}

fn flash_inputs(ui: &mut Ui, frame: &eframe::Frame, inputs: &mut Inputs) {
    ui.heading("刷写来源");
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut inputs.flash.kind, FlashKind::Xml, "XML");
        ui.selectable_value(
            &mut inputs.flash.kind,
            FlashKind::GeneratePackage,
            "准备固件包",
        );
        ui.selectable_value(
            &mut inputs.flash.kind,
            FlashKind::ReopenPackage,
            "重开固件包",
        );
    });
    match inputs.flash.kind {
        FlashKind::Xml => {
            path_list(ui, frame, "Program XML", &mut inputs.flash.programs);
            path_list(ui, frame, "Patch XML", &mut inputs.flash.patches);
            egui::ComboBox::from_id_salt("raw_size")
                .selected_text(match inputs.flash.raw_size {
                    RawSize::Declared => "RAW 按 XML 声明范围",
                    RawSize::File => "RAW 按文件长度",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut inputs.flash.raw_size,
                        RawSize::Declared,
                        "RAW 按 XML 声明范围",
                    );
                    ui.selectable_value(
                        &mut inputs.flash.raw_size,
                        RawSize::File,
                        "RAW 按文件长度",
                    );
                });
        }
        FlashKind::GeneratePackage | FlashKind::ReopenPackage => {
            path_field(
                ui,
                frame,
                "原固件包目录",
                &mut inputs.flash.package,
                Picker::Folder,
            );
            let label = ui.label("NV ID");
            ui.add(
                TextEdit::singleline(&mut inputs.flash.nv_id)
                    .hint_text("来自 META/super_def.<ID>.json"),
            )
            .labelled_by(label.id);
            optional_path(
                ui,
                frame,
                "刷写配置（可选）",
                &mut inputs.flash.config,
                Picker::File,
            );
            if inputs.flash.kind == FlashKind::GeneratePackage {
                path_field(
                    ui,
                    frame,
                    "新准备目录",
                    &mut inputs.flash.generated,
                    Picker::Folder,
                );
            } else {
                path_field(
                    ui,
                    frame,
                    "已准备目录",
                    &mut inputs.flash.prepared,
                    Picker::Folder,
                );
            }
        }
    }
    egui::CollapsingHeader::new("刷写后动作").show(ui, |ui| {
        let mut set_boot = inputs.flash.boot_lun.is_some();
        if ui.checkbox(&mut set_boot, "设置启动 LUN").changed() {
            inputs.flash.boot_lun = set_boot.then_some(0);
        }
        if let Some(lun) = &mut inputs.flash.boot_lun {
            ui.add(egui::DragValue::new(lun).range(0..=255));
        }
        ui.checkbox(&mut inputs.flash.reboot, "重启设备");
    });
}

fn path_list(ui: &mut Ui, frame: &eframe::Frame, title: &str, paths: &mut Vec<PathBuf>) {
    ui.push_id(title, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong(title);
            if ui.button("添加…").clicked()
                && let Some(selected) = rfd::FileDialog::new()
                    .set_parent(frame)
                    .set_title(title)
                    .add_filter("XML", &["xml"])
                    .pick_files()
            {
                paths.extend(selected);
            }
        });
        let mut edit = None;
        for (index, path) in paths.iter().enumerate() {
            ui.push_id(index, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(Label::new(format!("{} · {}", index + 1, path.display())).truncate());
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(index > 0, Button::new("上移")).clicked() {
                        edit = Some((index, Some(index - 1)));
                    } else if ui
                        .add_enabled(index + 1 < paths.len(), Button::new("下移"))
                        .clicked()
                    {
                        edit = Some((index, Some(index + 1)));
                    } else if ui.button("移除").clicked() {
                        edit = Some((index, None));
                    }
                });
            });
        }
        if let Some((from, to)) = edit {
            if let Some(to) = to {
                paths.swap(from, to);
            } else {
                paths.remove(from);
            }
        }
    });
}

pub(crate) fn verify_label(verify: Verify) -> &'static str {
    match verify {
        Verify::BestEffort => "尽力回读",
        Verify::Required => "要求完整回读验证",
        Verify::None => "跳过回读",
    }
}

fn center(app: &mut DesktopApp, ui: &mut Ui) {
    ui.heading(match app.mode {
        Mode::Backup => "设备分区",
        Mode::Restore => "恢复映射",
        Mode::Flash => "计划操作",
    });
    if app.mode != Mode::Flash {
        let mut scope = app.inputs().lun_scope.clone();
        ui.horizontal_wrapped(|ui| {
            let idle = !app.busy(Slot::Device) && !app.disconnecting && !app.closing;
            if ui
                .add_enabled(
                    idle,
                    TextEdit::singleline(&mut scope)
                        .desired_width(175.0)
                        .hint_text("LUN，留空扫描可发现范围"),
                )
                .changed()
            {
                let mut next = app.inputs().clone();
                next.lun_scope = scope;
                app.update_inputs(next);
            }
            if ui
                .add_enabled(idle && app.session().is_some(), Button::new("检查分区"))
                .clicked()
            {
                let result = app.inspect();
                app.remember_error(result);
            }
        });
        partition_controls(ui, app);
    }
    ui.horizontal_wrapped(|ui| {
        let reason = app.blocked_reason();
        if ui
            .add_enabled(reason.is_none(), primary_button(app.mode.action()))
            .clicked()
        {
            let result = app.start();
            app.remember_error(result);
        }
        if let Some(reason) = reason {
            ui.small(reason.to_string());
        }
    });
    ui.separator();
    if let Some(summary) = app.pending_write() {
        if let Some(preparation) = &summary.package {
            exclusions(ui, preparation);
        }
        work_table(ui, &summary.items, false);
    } else if app.mode != Mode::Flash {
        partition_table(ui, app);
    } else if let Some(ResultData::Preparation { preparation }) = app.offline_data() {
        ui.label(format!(
            "准备完成 · NV {} · 项目 {}",
            preparation.origin.nv_id, preparation.origin.project
        ));
        ui.label(preparation.manifest.display().to_string());
        exclusions(ui, preparation);
    } else if let Some(result) = app
        .result(Slot::Device)
        .filter(|result| result.command.as_deref() == Some("flash"))
    {
        work_table(ui, &result.items, true);
    } else {
        ui.label("准备刷写后显示按执行顺序排列的操作。");
    }
}

fn partition_controls(ui: &mut Ui, app: &mut DesktopApp) {
    let mut filter = app.partitions.filter.clone();
    let mut descending = app.partitions.descending;
    let mut changed = false;
    let selection_enabled = !app.busy(Slot::Device) && !app.closing && app.snapshot().is_some();
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                TextEdit::singleline(&mut filter)
                    .hint_text("筛选分区名称")
                    .desired_width(140.0),
            )
            .changed();
        if ui
            .button(if descending {
                "名称降序"
            } else {
                "名称升序"
            })
            .clicked()
        {
            descending = !descending;
            changed = true;
        }
        if ui
            .add_enabled(
                selection_enabled && app.mode == Mode::Backup,
                Button::new("全选已读取分区"),
            )
            .on_hover_text("包括被筛选隐藏的分区")
            .clicked()
        {
            app.select_all_partitions();
        }
        if ui
            .add_enabled(
                selection_enabled && !app.selected().is_empty(),
                Button::new("清空选择"),
            )
            .clicked()
        {
            app.clear_partitions();
        }
    });
    if changed {
        app.filter_partitions(filter, descending);
    }
    if let Some(snapshot) = app.snapshot() {
        ui.small(format!(
            "LUN {:?} · {} 条可见 · 已选 {}",
            snapshot.luns,
            app.partitions.visible.len(),
            app.selected().len()
        ));
    }
}

fn partition_table(ui: &mut Ui, app: &mut DesktopApp) {
    let Some(snapshot) = app.snapshot() else {
        ui.label("连接设备后检查 GPT，读取可选分区。");
        return;
    };
    if app.partitions.visible.is_empty() {
        ui.label("没有匹配的分区。");
        return;
    }
    let mut selection = None;
    let enabled = !app.busy(Slot::Device)
        && !app.closing
        && !(app.mode == Mode::Restore
            && app.inputs().restore.kind == RestoreKind::Directory
            && app.inputs().restore.all);
    ScrollArea::horizontal()
        .id_salt("partition_columns")
        .show(ui, |ui| {
            ui.set_min_width(420.0);
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::exact(30.0))
                .column(Column::initial(145.0).at_least(100.0).clip(true))
                .column(Column::exact(43.0))
                .column(Column::initial(90.0).clip(true))
                .column(Column::remainder().at_least(70.0).clip(true))
                .header(28.0, |mut header| {
                    for name in ["选中", "分区", "LUN", "起始扇区", "扇区数"] {
                        header.col(|ui| {
                            ui.strong(name);
                        });
                    }
                })
                .body(|body| {
                    body.rows(25.0, app.partitions.visible.len(), |mut row| {
                        let key = &snapshot.partitions[app.partitions.visible[row.index()]];
                        row.col(|ui| {
                            let mut selected = app.selected().contains(key);
                            if ui
                                .add_enabled(enabled, egui::Checkbox::without_text(&mut selected))
                                .changed()
                            {
                                selection = Some((key.clone(), selected));
                            }
                        });
                        row.col(|ui| {
                            ui.add(Label::new(&key.name).truncate());
                        });
                        row.col(|ui| {
                            ui.label(key.lun.to_string());
                        });
                        row.col(|ui| {
                            ui.add(Label::new(&key.start_sector).truncate());
                        });
                        row.col(|ui| {
                            ui.add(Label::new(&key.sectors).truncate());
                        });
                    });
                });
        });
    if let Some((key, selected)) = selection {
        if selected && app.mode == Mode::Restore && app.inputs().restore.kind == RestoreKind::Image
        {
            app.clear_partitions();
        }
        app.select_partition(&key, selected);
    }
}

fn work_table(ui: &mut Ui, items: &[Item], result: bool) {
    if items.is_empty() {
        return;
    }
    ScrollArea::horizontal()
        .id_salt("work_columns")
        .show(ui, |ui| {
            ui.set_min_width(600.0);
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::exact(35.0))
                .column(Column::initial(84.0).clip(true))
                .column(Column::initial(108.0).clip(true))
                .column(Column::initial(98.0).clip(true))
                .column(Column::initial(160.0).clip(true))
                .column(Column::remainder().at_least(130.0).clip(true))
                .header(28.0, |mut header| {
                    for name in [
                        "顺序",
                        "操作",
                        "目标 LUN 与 slot",
                        "目标",
                        "起始位置与长度",
                        if result { "结果" } else { "输入" },
                    ] {
                        header.col(|ui| {
                            ui.strong(name);
                        });
                    }
                })
                .body(|body| {
                    body.rows(26.0, items.len(), |mut row| {
                        let index = row.index();
                        let item = &items[index];
                        row.col(|ui| {
                            ui.label((index + 1).to_string());
                        });
                        row.col(|ui| {
                            ui.add(Label::new(&item.operation).truncate());
                        });
                        row.col(|ui| {
                            let location = item_location(item);
                            ui.add(Label::new(&location).truncate())
                                .on_hover_text(location);
                        });
                        row.col(|ui| {
                            ui.add(Label::new(item.partition.as_deref().unwrap_or("—")).truncate());
                        });
                        row.col(|ui| {
                            let range = item_range(item);
                            ui.add(Label::new(&range).truncate()).on_hover_text(range);
                        });
                        row.col(|ui| {
                            let summary = if result {
                                format!(
                                    "{} · {} · {}",
                                    execution_label(item.execution),
                                    write_label(item.write),
                                    verification_label(item.verification)
                                )
                            } else {
                                item.source
                                    .as_deref()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|| "—".into())
                            };
                            ui.add(Label::new(&summary).truncate())
                                .on_hover_text(summary);
                        });
                    });
                });
        });
}

pub(crate) fn item_range(item: &Item) -> String {
    let Some(length) = &item.transfer_bytes else {
        return "—".into();
    };
    if let Some(start) = &item.start_byte {
        format!("起始 {start} · {length} 字节")
    } else {
        format!("起始位置未解析 · {length} 字节")
    }
}

pub(crate) fn item_location(item: &Item) -> String {
    let Some(lun) = item.lun else {
        return "—".into();
    };
    match item.slot {
        Some(slot) => format!("LUN {lun} · slot {slot}"),
        None => format!("LUN {lun}"),
    }
}

pub(crate) fn exclusions(ui: &mut Ui, preparation: &Preparation) {
    if !preparation.exclusions.is_empty() {
        egui::CollapsingHeader::new(format!("排除 {} 项", preparation.exclusions.len())).show(
            ui,
            |ui| {
                for item in &preparation.exclusions {
                    ui.small(format!(
                        "{}:{} · {}",
                        item.lun,
                        item.label,
                        item.reason.as_deref().unwrap_or("排除")
                    ));
                }
            },
        );
    }
}

fn outcome(status: Status) -> &'static str {
    match status {
        Status::Success => "已完成",
        Status::Stopped => "已停止",
        Status::InputError => "输入或确认被拒绝",
        Status::Unsupported => "能力不支持",
        Status::ExecutionFailed => "执行失败",
        Status::VerificationIncomplete => "验证未完成",
        Status::UnknownWrite => "写入结果未知",
        Status::InputChanged => "输入已变化",
    }
}

fn execution_label(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::NotAttempted => "not_attempted",
        ExecutionState::Done => "done",
        ExecutionState::Failed => "failed",
        ExecutionState::Skipped => "skipped",
    }
}

fn write_label(state: WriteState) -> &'static str {
    match state {
        WriteState::NotApplicable => "not_applicable",
        WriteState::NotAttempted => "not_attempted",
        WriteState::Acknowledged => "acknowledged",
        WriteState::Failed => "failed",
        WriteState::Unknown => "unknown",
    }
}

fn verification_label(state: VerificationState) -> &'static str {
    match state {
        VerificationState::NotApplicable => "not_applicable",
        VerificationState::NotRequested => "not_requested",
        VerificationState::Matched => "matched",
        VerificationState::Mismatch => "mismatch",
        VerificationState::Incomplete => "incomplete",
    }
}

fn result_details(ui: &mut Ui, result: &OperationResult) {
    ui.strong(outcome(result.status));
    if let Some(command) = &result.command {
        ui.small(format!("{command} · 会话 {:?}", result.session.disposition));
    }
    if let Some(reason) = result.session.reason {
        ui.small(format!("会话原因 {reason}"));
    }
    for step in &result.session.completed_initialization_steps {
        use oplus_edl_core::model::InitializationStep;
        let label = match step {
            InitializationStep::SaharaUpload => "Sahara 加载器上传",
            InitializationStep::FirehoseWelcome => "Firehose 欢迎应答",
            InitializationStep::SignedDigestAck => "签名摘要 ACK",
            InitializationStep::DigestSignatureAck => "DIGEST 签名 ACK",
            InitializationStep::Sha256initAck => "SHA-256 初始化 ACK",
            InitializationStep::ConfigureAck => "存储配置 ACK",
        };
        ui.small(format!("已完成 {label}"));
    }
    if let Some(reason) = result.stop_reason {
        ui.small(format!("停止原因 {reason:?}"));
    }
    if let Some(reason) = &result.coverage.reason {
        ui.small(reason);
    }
    for (label, values) in [("错误", &result.errors), ("警告", &result.warnings)] {
        for error in values {
            ui.colored_label(WARNING, format!("{label} · {}", error.message));
        }
    }
    if !result.outputs.is_empty() {
        egui::CollapsingHeader::new(format!("保留 {} 项输出", result.outputs.len())).show(
            ui,
            |ui| {
                for output in &result.outputs {
                    ui.small(format!("{:?} · {}", output.state, output.path.display()));
                }
            },
        );
    }
    if !result.items.is_empty() {
        let done = result
            .items
            .iter()
            .filter(|item| item.execution == ExecutionState::Done)
            .count();
        let acknowledged = result
            .items
            .iter()
            .filter(|item| item.write == WriteState::Acknowledged)
            .count();
        ui.small(format!(
            "完成 {done}/{} · 写入 ACK {acknowledged}",
            result.items.len()
        ));
        egui::CollapsingHeader::new("项目与回读结果").show(ui, |ui| {
            for item in &result.items {
                ui.group(|ui| {
                    let target = item.partition.as_deref().map_or_else(
                        || item.operation.clone(),
                        |partition| format!("{} {partition}", item.operation),
                    );
                    ui.small(format!(
                        "#{} {target} · {} · {}",
                        item.id,
                        item_location(item),
                        item_range(item)
                    ));
                    ui.small(format!(
                        "执行 {} · 写入 {} · 验证 {} · 已传输 {} 字节",
                        execution_label(item.execution),
                        write_label(item.write),
                        verification_label(item.verification),
                        item.transferred_bytes.as_deref().unwrap_or("—")
                    ));
                    for range in &item.ranges {
                        ui.small(format!(
                            "回读起始 {} · {} 字节 · {}",
                            range.start_byte.as_deref().unwrap_or("未解析"),
                            range.length_bytes.as_deref().unwrap_or("未知"),
                            verification_label(range.verification)
                        ));
                        if let Some(reason) = &range.reason {
                            ui.small(if reason == "byte_difference_observed" {
                                "已观察到字节差异"
                            } else {
                                reason
                            });
                        }
                    }
                    for error in &item.errors {
                        ui.colored_label(WARNING, &error.message);
                    }
                });
            }
        });
    }
}

fn status(app: &mut DesktopApp, ui: &mut Ui) {
    ScrollArea::vertical()
        .id_salt("status_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.heading("设备状态");
            Frame::new()
                .inner_margin(10)
                .fill(WASH)
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(5.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(device) = &app.connected {
                        ui.label(
                            RichText::new(device.target.device.as_deref().unwrap_or("已连接"))
                                .strong()
                                .color(ACCENT),
                        );
                        ui.small(format!(
                            "{} · {} · 配置 {} 字节/扇区",
                            device.target.backend.as_deref().unwrap_or("—"),
                            device.target.memory.as_deref().unwrap_or("—"),
                            device.sector_size
                        ));
                    } else if app.disconnecting {
                        ui.label("正在释放会话");
                    } else {
                        ui.label("未连接");
                    }
                });
            ui.separator();
            ui.heading("最近结果");
            ui.horizontal_wrapped(|ui| {
                for (label, slot) in [("设备", Slot::Device), ("离线", Slot::Offline)] {
                    if ui
                        .add_enabled(
                            app.result(slot).is_some(),
                            Button::new(label).selected(app.result_slot == slot),
                        )
                        .clicked()
                    {
                        app.result_slot = slot;
                    }
                }
            });
            if let Some(result) = app.result(app.result_slot) {
                result_details(ui, result);
            } else {
                ui.small("暂无任务结果");
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.heading("日志");
                if ui
                    .add_enabled(!app.logs.is_empty(), Button::new("清空"))
                    .clicked()
                {
                    app.clear_log();
                }
                if ui
                    .add_enabled(
                        app.diagnostic_export.is_none() && !app.closing,
                        Button::new("导出诊断…"),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("oplus-edl-diagnostics.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    && let Err(error) = app.export_diagnostics(path)
                {
                    app.notice = Some(format!("导出失败 · {error}"));
                }
            });
            if app.dropped_logs > 0 {
                ui.small(format!("已丢弃 {} 条较早日志", app.dropped_logs));
            }
            for event in app.logs.iter().rev() {
                ui.small(&event.message);
            }
        });
}

fn phase_label(phase: &str) -> &str {
    match phase {
        "discovery" => "发现设备",
        "acquisition" => "获取资产",
        "preparation" => "准备输入",
        "initialization" => "初始化",
        "inspection" => "检查分区",
        "awaiting_consent" => "等待确认",
        "read" => "读取",
        "write" => "写入",
        "verification" => "回读验证",
        "finalization" => "执行后续动作",
        _ => phase,
    }
}

fn progress(app: &mut DesktopApp, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        if app.closing {
            ui.strong("正在关闭");
        }
        for slot in [Slot::Device, Slot::Offline] {
            if let Some((name, status)) = app.current_job(slot) {
                ui.strong(name);
                let stopping = app.stop_requested(slot) || app.closing;
                if ui.add_enabled(!stopping, Button::new("停止")).clicked() {
                    app.stop(slot);
                }
                if app.stop_requested(slot) {
                    ui.colored_label(WARNING, "已请求停止");
                } else {
                    ui.label(phase_label(status.progress.phase));
                }
                match (status.progress.completed_bytes, status.progress.total_bytes) {
                    (Some(done), Some(total)) if total > 0 => {
                        ui.add(
                            egui::ProgressBar::new(done as f32 / total as f32)
                                .desired_width(114.0)
                                .fill(ACCENT)
                                .text(format!("{done} / {total}")),
                        );
                    }
                    _ => {
                        ui.spinner();
                    }
                }
            } else if app.stop_requested(slot) {
                let label = match slot {
                    Slot::Device => "设备 · 已请求停止",
                    Slot::Offline => "离线 · 已请求停止",
                };
                ui.colored_label(WARNING, label);
            }
        }
        if !app.busy(Slot::Device) && !app.busy(Slot::Offline) && !app.closing {
            ui.small(if app.disconnecting {
                "断开会话"
            } else {
                "就绪"
            });
        }
    });
    if app.busy(Slot::Device) || app.busy(Slot::Offline) || app.disconnecting || app.closing {
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }
}

#[cfg(test)]
fn view_contains(output: &egui::FullOutput, expected: &str) -> bool {
    use egui::epaint::Shape;
    fn contains(shape: &Shape, expected: &str) -> bool {
        match shape {
            Shape::Text(text) => text.galley.job.text.contains(expected),
            Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, expected)),
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, expected))
}

#[cfg(test)]
#[test]
fn terminal_view_renders_session_reason_and_completed_steps_from_the_result() {
    use oplus_edl_core::{model::InitializationStep, result::SessionDisposition};
    let mut result = OperationResult::new(Some("connect"), None);
    result.error(Status::ExecutionFailed, "configure failed");
    result.session.disposition = SessionDisposition::Closed;
    result.session.reason = Some("exchange_failed");
    result.session.completed_initialization_steps = vec![
        InitializationStep::SaharaUpload,
        InitializationStep::FirehoseWelcome,
    ];
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| result_details(ui, &result));
    output.textures_delta.clear();
    for expected in ["exchange_failed", "Sahara 加载器上传", "Firehose 欢迎应答"] {
        assert!(view_contains(&output, expected), "{expected}");
    }
    assert!(!view_contains(&output, "存储配置 ACK"));
}

#[cfg(test)]
#[test]
fn backup_result_view_renders_completed_gpt_and_an_unattempted_partition() {
    let mut result = OperationResult::new(Some("backup"), None);
    let mut gpt = Item::read(0, None, 512, 512);
    gpt.execution = ExecutionState::Done;
    gpt.transferred_bytes = Some("512".into());
    let mut partition = Item::read(0, Some("boot".into()), 32768, 1024);
    partition.reject(Status::InputError, "output or partial already exists");
    result.items = vec![gpt, partition];
    result.summarize();

    let ctx = egui::Context::default();
    ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| result_details(ui, &result));
    output.textures_delta.clear();
    for expected in [
        "#1 read · LUN 0 · 起始 512 · 512 字节",
        "执行 done · 写入 not_applicable · 验证 not_applicable · 已传输 512 字节",
        "#2 read boot · LUN 0 · 起始 32768 · 1024 字节",
        "执行 not_attempted · 写入 not_applicable · 验证 not_applicable · 已传输 0 字节",
        "output or partial already exists",
    ] {
        assert!(view_contains(&output, expected), "{expected}");
    }
}

#[cfg(test)]
#[test]
fn result_view_renders_readback_subranges_and_observed_difference() {
    use oplus_edl_core::result::Range;
    let mut result = OperationResult::new(Some("flash"), Some("best-effort"));
    let mut item = Item::read(0, Some("boot".into()), 32768, 1536);
    item.operation = "program".into();
    item.execution = ExecutionState::Done;
    item.write = WriteState::Acknowledged;
    item.verification = VerificationState::Incomplete;
    item.ranges = [
        ("32768", VerificationState::Matched, None),
        (
            "33280",
            VerificationState::Incomplete,
            Some("byte_difference_observed"),
        ),
        ("33792", VerificationState::Mismatch, None),
    ]
    .into_iter()
    .map(|(start, state, reason)| Range {
        start_byte: Some(start.into()),
        length_bytes: Some("512".into()),
        verification: state,
        reason: reason.map(str::to_owned),
    })
    .collect();
    result.items.push(item);
    result.summarize();
    let ctx = egui::Context::default();
    ctx.memory_mut(|memory| memory.set_everything_is_visible(true));
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| result_details(ui, &result));
    output.textures_delta.clear();
    for expected in [
        "回读起始 32768 · 512 字节 · matched",
        "回读起始 33280 · 512 字节 · incomplete",
        "已观察到字节差异",
        "回读起始 33792 · 512 字节 · mismatch",
    ] {
        assert!(view_contains(&output, expected), "{expected}");
    }
}
