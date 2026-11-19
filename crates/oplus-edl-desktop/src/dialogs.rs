use eframe::egui::{self, TextEdit};
use oplus_edl_core::model::{Decision, PreparedSummary};
use std::path::{Path, PathBuf};

use crate::views;

#[derive(Clone, Copy)]
pub(crate) enum Picker {
    File,
    Folder,
}

pub(crate) fn path_field(
    ui: &mut egui::Ui,
    frame: &eframe::Frame,
    label: &str,
    path: &mut PathBuf,
    picker: Picker,
) {
    ui.push_id(label, |ui| {
        let caption = ui.label(label);
        ui.horizontal(|ui| {
            const BROWSE_WIDTH: f32 = 62.0;
            let height = ui.spacing().interact_size.y;
            let width =
                (ui.available_width() - BROWSE_WIDTH - ui.spacing().item_spacing.x).max(40.0);
            let mut display = path.to_string_lossy().into_owned();
            let editable = path.to_str().is_some();
            if ui
                .add_sized(
                    [width, height],
                    TextEdit::singleline(&mut display)
                        .id_salt("path")
                        .interactive(editable)
                        .hint_text("输入路径"),
                )
                .labelled_by(caption.id)
                .changed()
            {
                apply_edit(path, display);
            }
            if ui
                .add_sized([BROWSE_WIDTH, height], egui::Button::new("浏览…"))
                .clicked()
            {
                let mut dialog = rfd::FileDialog::new().set_parent(frame).set_title(label);
                let directory = match picker {
                    Picker::File => path.parent(),
                    Picker::Folder => Some(path.as_path()),
                };
                if let Some(directory) = directory.filter(|path| path.is_absolute()) {
                    dialog = dialog.set_directory(directory);
                }
                apply_pick(
                    path,
                    match picker {
                        Picker::File => dialog.pick_file(),
                        Picker::Folder => dialog.pick_folder(),
                    },
                );
            }
        });
        if path.to_str().is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.small("此路径可用浏览替换，或清空后输入完整路径。");
                if ui.small_button("清空路径").clicked() {
                    path.clear();
                }
            });
        }
    });
}

fn apply_edit(path: &mut PathBuf, edited: String) {
    if path.to_str().is_some() {
        *path = PathBuf::from(edited);
    }
}

fn apply_pick(path: &mut PathBuf, picked: Option<PathBuf>) {
    if let Some(picked) = picked {
        *path = picked;
    }
}

pub(crate) fn optional_path(
    ui: &mut egui::Ui,
    frame: &eframe::Frame,
    label: &str,
    path: &mut Option<PathBuf>,
    picker: Picker,
) {
    let mut edited = path.clone().unwrap_or_default();
    path_field(ui, frame, label, &mut edited, picker);
    *path = (!edited.as_os_str().is_empty()).then_some(edited);
}

pub(crate) fn confirm_write(ctx: &egui::Context, summary: &PreparedSummary) -> Option<Decision> {
    let window_size = ctx.content_rect().size();
    let response = egui::Modal::new(egui::Id::unique("write_summary")).show(ctx, |ui| {
        ui.set_width((window_size.x - 44.0).clamp(235.0, 595.0));
        ui.heading("确认设备写入");
        if let Some(target) = &summary.target {
            ui.label(format!(
                "设备 {} · {} · LUN {:?}",
                target.device.as_deref().unwrap_or("—"),
                target.memory.as_deref().unwrap_or("—"),
                target.luns
            ));
        }
        ui.label(format!("验证策略 {}", views::verify_label(summary.verify)));
        if let Some(reason) = &summary.coverage.reason {
            ui.label(reason);
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height((window_size.y * 0.53).max(88.0))
            .show(ui, |ui| {
                if let Some(preparation) = &summary.package {
                    ui.strong(format!(
                        "固件包项目 {} · NV {}",
                        preparation.origin.project, preparation.origin.nv_id
                    ));
                    ui.label(preparation.origin.root.display().to_string());
                    views::exclusions(ui, preparation);
                }
                for (index, item) in summary.items.iter().enumerate() {
                    let source = item
                        .source
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default();
                    ui.label(format!(
                        "{}. {} · {} · {} · {}",
                        index + 1,
                        item.operation,
                        views::item_location(item),
                        item.partition.as_deref().unwrap_or("—"),
                        views::item_range(item)
                    ));
                    if !source.is_empty() {
                        ui.label(format!("  {source}"));
                    }
                    if let Some(reason) = &item.reason {
                        ui.small(reason);
                    }
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("取消").clicked() {
                Some(Decision::Decline)
            } else if ui.add(views::primary_button("确认写入")).clicked() {
                Some(Decision::Approve)
            } else {
                None
            }
        })
        .inner
    });
    response
        .inner
        .or_else(|| response.should_close().then_some(Decision::Decline))
}

pub(crate) fn confirm_identity(ctx: &egui::Context, path: &Path) -> Option<bool> {
    let response = egui::Modal::new(egui::Id::unique("installation_identity")).show(ctx, |ui| {
        ui.heading("创建新的安装身份");
        ui.label(path.display().to_string());
        ui.horizontal(|ui| {
            if ui.button("取消").clicked() {
                Some(false)
            } else if ui.add(views::primary_button("创建")).clicked() {
                Some(true)
            } else {
                None
            }
        })
        .inner
    });
    response
        .inner
        .or_else(|| response.should_close().then_some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_picker_preserves_the_selected_path() {
        let original = PathBuf::from("固件资料/测试 镜像.img");
        let mut selected = original.clone();
        apply_pick(&mut selected, None);
        assert_eq!(selected, original);
    }

    #[test]
    fn confirmation_distinguishes_equal_lengths_and_marks_unresolved_positions() {
        use oplus_edl_core::result::Item;
        let mut item = Item::read(0, None, 512, 512);
        item.operation = "patch".to_owned();
        assert_eq!(views::item_range(&item), "起始 512 · 512 字节");
        item.start_byte = Some("1024".into());
        assert_eq!(views::item_range(&item), "起始 1024 · 512 字节");
        item.start_byte = None;
        assert_eq!(views::item_range(&item), "起始位置未解析 · 512 字节");
        item.slot = Some(1);
        assert_eq!(views::item_location(&item), "LUN 0 · slot 1");
        item.slot = Some(2);
        assert_eq!(views::item_location(&item), "LUN 0 · slot 2");
    }

    #[cfg(unix)]
    #[test]
    fn a_picker_keeps_the_original_non_unicode_path() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let original = PathBuf::from(OsString::from_vec(b"image-\xff.img".to_vec()));
        let mut selected = PathBuf::new();
        apply_pick(&mut selected, Some(original.clone()));
        apply_edit(&mut selected, "edited replacement fragment".into());
        apply_pick(&mut selected, None);
        assert_eq!(selected, original);
        selected.clear();
        apply_edit(&mut selected, "complete replacement.img".into());
        assert_eq!(selected, PathBuf::from("complete replacement.img"));
    }
}
