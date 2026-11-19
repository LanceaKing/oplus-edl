use crate::app::{DesktopApp, FlashKind, Inputs, Mode, RestoreKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub window_size: [f32; 2],
    zoom: f32,
    show_inputs: bool,
    show_status: bool,
    mode: Mode,
    flash_kind: FlashKind,
    restore_kind: RestoreKind,
    paths: BTreeMap<String, OsString>,
    programs: Vec<OsString>,
    patches: Vec<OsString>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            window_size: [1280.0, 800.0],
            zoom: 1.0,
            show_inputs: true,
            show_status: true,
            mode: Mode::default(),
            flash_kind: FlashKind::default(),
            restore_kind: RestoreKind::default(),
            paths: BTreeMap::new(),
            programs: Vec::new(),
            patches: Vec::new(),
        }
    }
}

fn supported_zoom(zoom: f32) -> f32 {
    // Match the keyboard zoom limits in the pinned egui version.
    if (0.2..=5.0).contains(&zoom) {
        zoom
    } else {
        1.0
    }
}

impl Preferences {
    pub fn load() -> Self {
        preference_path()
            .and_then(|path| Self::read(&path).ok())
            .unwrap_or_default()
    }

    fn read(path: &Path) -> io::Result<Self> {
        use std::io::Read;
        let bytes = fs::File::open(path)?.take(1024 * 1024 + 1);
        let mut preferences: Self = serde_json::from_reader(bytes)?;
        let width = &mut preferences.window_size[0];
        *width = if width.is_finite() {
            width.clamp(640.0, 4096.0)
        } else {
            1280.0
        };
        let height = &mut preferences.window_size[1];
        *height = if height.is_finite() {
            height.clamp(480.0, 3072.0)
        } else {
            800.0
        };
        preferences.zoom = supported_zoom(preferences.zoom);
        Ok(preferences)
    }

    pub(crate) fn apply(self, app: &mut DesktopApp) {
        let path = |key: &str| self.paths.get(key).map(PathBuf::from);
        let mut inputs = Inputs {
            loader: path("loader").unwrap_or_default(),
            signed_digest: path("signed_digest"),
            digest_signature: path("digest_signature"),
            ..Inputs::default()
        };
        inputs.backup.output = path("backup_output").unwrap_or_default();
        inputs.restore.image = path("restore_image").unwrap_or_default();
        inputs.restore.directory = path("restore_directory").unwrap_or_default();
        inputs.flash.programs = self.programs.into_iter().map(PathBuf::from).collect();
        inputs.flash.patches = self.patches.into_iter().map(PathBuf::from).collect();
        inputs.flash.package = path("package").unwrap_or_default();
        inputs.flash.config = path("flash_config");
        inputs.flash.generated = path("prepare_output").unwrap_or_default();
        inputs.flash.prepared = path("prepared").unwrap_or_default();
        inputs.cloud.config = path("cloud_config").unwrap_or_default();
        inputs.cloud.output = path("cloud_output").unwrap_or_default();
        inputs.cloud.base = path("cloud_base");
        inputs.flash.kind = self.flash_kind;
        inputs.restore.kind = self.restore_kind;
        app.update_inputs(inputs);
        app.set_mode(self.mode);
        app.show_inputs = self.show_inputs;
        app.show_status = self.show_status;
        app.window_size = self.window_size;
        app.zoom = self.zoom;
    }

    pub(crate) fn capture(app: &DesktopApp) -> Self {
        let input = app.inputs();
        let paths = [
            ("loader", Some(input.loader.as_path())),
            ("signed_digest", input.signed_digest.as_deref()),
            ("digest_signature", input.digest_signature.as_deref()),
            ("backup_output", Some(input.backup.output.as_path())),
            ("restore_image", Some(input.restore.image.as_path())),
            ("restore_directory", Some(input.restore.directory.as_path())),
            ("package", Some(input.flash.package.as_path())),
            ("flash_config", input.flash.config.as_deref()),
            ("prepare_output", Some(input.flash.generated.as_path())),
            ("prepared", Some(input.flash.prepared.as_path())),
            ("cloud_config", Some(input.cloud.config.as_path())),
            ("cloud_output", Some(input.cloud.output.as_path())),
            ("cloud_base", input.cloud.base.as_deref()),
        ]
        .into_iter()
        .filter_map(|(key, path)| {
            let path = path.filter(|path| !path.as_os_str().is_empty())?;
            Some((key.to_owned(), path.as_os_str().to_owned()))
        })
        .collect();
        Self {
            paths,
            programs: input
                .flash
                .programs
                .iter()
                .map(|path| path.as_os_str().to_owned())
                .collect(),
            patches: input
                .flash
                .patches
                .iter()
                .map(|path| path.as_os_str().to_owned())
                .collect(),
            show_inputs: app.show_inputs,
            show_status: app.show_status,
            mode: app.mode,
            flash_kind: input.flash.kind,
            restore_kind: input.restore.kind,
            window_size: app.window_size,
            zoom: supported_zoom(app.zoom),
        }
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        if let Some(path) = preference_path() {
            self.write(&path)?;
        }
        Ok(())
    }

    fn write(&self, path: &Path) -> io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::other("preferences have no directory"))?;
        fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer_pretty(file.as_file_mut(), self)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}

fn preference_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    Some(base?.join("oplus-edl/desktop.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oplus_edl_core::{CoreService, HostServices};

    #[test]
    fn preferences_restore_intermediate_keyboard_zoom_and_logical_size_independently() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("desktop.json");
        let create_app = || DesktopApp::with_service(CoreService::new(HostServices::default()));
        let mut app = create_app();
        app.window_size = [1165.0, 732.0];
        for (input_zoom, restored_zoom) in [(0.2, 0.2), (1.1, 1.1), (5.0, 5.0), (7.0, 1.0)] {
            app.zoom = input_zoom;
            Preferences::capture(&app).write(&path).unwrap();
            let mut restarted = create_app();
            Preferences::read(&path).unwrap().apply(&mut restarted);
            assert_eq!(restarted.zoom, restored_zoom);
            assert_eq!(restarted.window_size, app.window_size);
        }
    }

    #[test]
    fn preferences_round_trip_typed_paths_without_consent_or_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("desktop.json");
        let mut app = DesktopApp::with_service(CoreService::new(HostServices::default()));
        let mut inputs = app.inputs().clone();
        inputs.cloud.base = Some(temp.path().join("固定基目录"));
        inputs.cloud.config = temp.path().join("cloud.json");
        inputs.flash.kind = FlashKind::ReopenPackage;
        inputs.flash.prepared = temp.path().join("prepared");
        inputs.device = Some("temporary-device-key".into());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            inputs.loader = PathBuf::from(OsString::from_vec(b"loader-\xff.elf".to_vec()));
        }
        app.update_inputs(inputs.clone());
        Preferences::capture(&app).write(&file).unwrap();
        Preferences::read(&file).unwrap().apply(&mut app);
        assert_eq!(app.inputs().cloud, inputs.cloud);
        assert_eq!(app.inputs().loader, inputs.loader);
        assert_eq!(app.inputs().flash.kind, FlashKind::ReopenPackage);
        assert_eq!(app.inputs().flash.prepared, inputs.flash.prepared);
        assert!(app.inputs().device.is_none());
        let json = fs::read_to_string(file).unwrap();
        assert!(!json.contains("temporary-device-key"));
        assert!(!json.contains("consent"));
    }
}
