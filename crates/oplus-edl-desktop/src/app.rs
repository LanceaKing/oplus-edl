use crate::{diagnostics, dialogs, preferences::Preferences, views};
use eframe::egui;
use oplus_edl_core::{
    CoreService, HostServices, JobHandle, OperationResult, StopReason, SubmitError,
    events::{Event, JobStatus},
    model::{
        Backend, CloudInput, Connection, Decision, FlashInput, IdentityMode, LoaderInput, Memory,
        OperationRequest, PackageArtifacts, PackageInput, PartitionKey, PartitionSelection,
        PartitionSelector, PartitionSnapshot, PartitionTargets, PreparationId, PreparedSummary,
        RawSize, Request, Scope, SessionConfig, SessionId, Verify,
    },
    result::{DeviceCandidate, ResultData, SessionDisposition, Status, Target},
    service::Slot,
};
use std::{
    collections::{HashSet, VecDeque},
    fmt, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Backup,
    Restore,
    Flash,
}

impl Mode {
    pub(crate) const ALL: [Self; 3] = [Self::Backup, Self::Restore, Self::Flash];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Backup => "备份",
            Self::Restore => "恢复",
            Self::Flash => "刷写",
        }
    }

    pub(crate) fn action(self) -> &'static str {
        match self {
            Self::Backup => "开始备份",
            Self::Restore => "准备恢复",
            Self::Flash => "准备刷写",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidePanel {
    Inputs,
    Status,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudFiles {
    pub config: PathBuf,
    pub output: PathBuf,
    pub base: Option<PathBuf>,
}

impl CloudFiles {
    fn request(&self, needs_output: bool) -> Result<CloudInput, ActionError> {
        if self.config.as_os_str().is_empty() {
            return Err(ActionError::Input("请选择云端配置文件"));
        }
        if needs_output && self.output.as_os_str().is_empty() {
            return Err(ActionError::Input("请输入新资产目录"));
        }
        Ok(CloudInput {
            config: self.config.clone(),
            output: self.output.clone(),
            base_directory: self.base.clone(),
            identity_mode: IdentityMode::OpenExisting,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackupInputs {
    pub output: PathBuf,
    pub overwrite: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreKind {
    #[default]
    Image,
    Directory,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreInputs {
    pub kind: RestoreKind,
    pub image: PathBuf,
    pub directory: PathBuf,
    pub all: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashKind {
    #[default]
    Xml,
    GeneratePackage,
    ReopenPackage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlashInputs {
    pub kind: FlashKind,
    pub programs: Vec<PathBuf>,
    pub patches: Vec<PathBuf>,
    pub raw_size: RawSize,
    pub package: PathBuf,
    pub nv_id: String,
    pub config: Option<PathBuf>,
    pub generated: PathBuf,
    pub prepared: PathBuf,
    pub boot_lun: Option<u8>,
    pub reboot: bool,
}

impl FlashInputs {
    fn package_input(&self) -> Result<PackageInput, ActionError> {
        if self.package.as_os_str().is_empty() {
            return Err(ActionError::Input("请选择原固件包目录"));
        }
        if self.nv_id.is_empty() || !self.nv_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ActionError::Input("请输入 NV 定义文件名中的数字 ID"));
        }
        Ok(PackageInput {
            root: self.package.clone(),
            nv_id: self.nv_id.clone(),
            config: self.config.clone(),
        })
    }

    fn flash_input(&self) -> Result<FlashInput, ActionError> {
        match self.kind {
            FlashKind::Xml => {
                if self.programs.is_empty() {
                    return Err(ActionError::Input("请按写入顺序添加 program XML"));
                }
                Ok(FlashInput::Xml {
                    programs: self.programs.clone(),
                    patches: self.patches.clone(),
                    raw_size: self.raw_size,
                })
            }
            FlashKind::GeneratePackage | FlashKind::ReopenPackage => {
                let input = self.package_input()?;
                let artifacts = if self.kind == FlashKind::GeneratePackage {
                    require_path(&self.generated, "请输入新的准备目录")?;
                    PackageArtifacts::Generate(self.generated.clone())
                } else {
                    require_path(&self.prepared, "请选择已准备的目录")?;
                    PackageArtifacts::Reopen(self.prepared.clone())
                };
                Ok(FlashInput::Package { input, artifacts })
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    pub backend: Option<Backend>,
    pub device: Option<String>,
    pub memory: Memory,
    pub sector_size: Option<usize>,
    pub wait_seconds: Option<u64>,
    pub loader: PathBuf,
    pub signed_digest: Option<PathBuf>,
    pub digest_signature: Option<PathBuf>,
    pub use_cloud: bool,
    pub cloud: CloudFiles,
    pub lun_scope: String,
    pub backup: BackupInputs,
    pub restore: RestoreInputs,
    pub flash: FlashInputs,
    pub verify: Verify,
}

impl Inputs {
    pub fn discovery_backend(&self) -> Backend {
        self.backend.unwrap_or_default()
    }

    fn connection(&self) -> Result<SessionConfig, ActionError> {
        let backend = self.discovery_backend();
        if backend == Backend::Serial && self.device.as_ref().is_none_or(|key| key.is_empty()) {
            return Err(ActionError::Input("串口连接需要明确的设备键"));
        }
        let loader = if self.use_cloud {
            LoaderInput::Cloud(self.cloud.request(true)?)
        } else {
            require_path(&self.loader, "请选择加载器")?;
            if self.digest_signature.is_some() && self.signed_digest.is_none() {
                return Err(ActionError::Input("DIGEST 签名需要配套签名摘要"));
            }
            LoaderInput::Local {
                loader: Some(self.loader.clone()),
                signed_digest: self.signed_digest.clone(),
                digest_signature: self.digest_signature.clone(),
            }
        };
        Ok(SessionConfig {
            backend,
            device: self.device.clone(),
            memory: self.memory,
            sector_size: self.sector_size,
            loader,
            wait_timeout: self.wait_seconds.map(Duration::from_secs),
        })
    }

    fn scope(&self) -> Result<Scope, ActionError> {
        if self.lun_scope.trim().is_empty() {
            return Ok(Scope::All);
        }
        let mut luns = Vec::new();
        for entry in self.lun_scope.split(',') {
            let lun: u8 = entry
                .trim()
                .parse()
                .map_err(|_| ActionError::Input("请输入由逗号分隔的 LUN 编号"))?;
            if luns.contains(&lun) {
                return Err(ActionError::Input("LUN 编号不能重复"));
            }
            luns.push(lun);
        }
        Ok(Scope::Selected(luns))
    }
}

fn require_path(path: &Path, message: &'static str) -> Result<(), ActionError> {
    if path.as_os_str().is_empty() {
        Err(ActionError::Input(message))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionError {
    Input(&'static str),
    Service(SubmitError),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(message) => f.write_str(message),
            Self::Service(error) => f.write_str(match error {
                SubmitError::Busy => "该工作槽仍有任务",
                SubmitError::InvalidRequest => "请求或确认已失效",
                SubmitError::StaleSession => "设备会话已结束",
                SubmitError::StaleSnapshot => "分区布局已变化，请重新检查",
                SubmitError::Unsupported => "当前构建不支持所选能力",
                SubmitError::WorkerStart => "无法启动任务工作线程",
            }),
        }
    }
}

impl std::error::Error for ActionError {}

impl From<SubmitError> for ActionError {
    fn from(error: SubmitError) -> Self {
        Self::Service(error)
    }
}

#[derive(Default)]
pub(crate) struct Partitions {
    pub(crate) snapshot: Option<PartitionSnapshot>,
    pub(crate) visible: Vec<usize>,
    pub(crate) selected: HashSet<PartitionKey>,
    retained_selection: HashSet<PartitionKey>,
    pub(crate) filter: String,
    pub(crate) descending: bool,
}

impl Partitions {
    fn replace(&mut self, snapshot: Option<PartitionSnapshot>) {
        let mut previous = std::mem::take(&mut self.selected);
        previous.extend(self.retained_selection.drain());
        if let Some(snapshot) = &snapshot
            && let Some(revision) = previous.iter().next().map(|key| key.snapshot_revision)
        {
            self.selected.extend(
                snapshot
                    .partitions
                    .iter()
                    .filter(|key| {
                        let mut old = (*key).clone();
                        old.snapshot_revision = revision;
                        previous.contains(&old)
                    })
                    .cloned(),
            );
        }
        self.snapshot = snapshot;
        self.refresh_view();
    }

    fn invalidate(&mut self) {
        self.retained_selection = std::mem::take(&mut self.selected);
        self.snapshot = None;
        self.visible.clear();
    }

    fn refresh_view(&mut self) {
        self.visible.clear();
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let query = self.filter.trim().to_lowercase();
        self.visible.extend(
            snapshot
                .partitions
                .iter()
                .enumerate()
                .filter(|(_, key)| key.name.to_lowercase().contains(&query))
                .map(|(index, _)| index),
        );
        self.visible.sort_by(|&a, &b| {
            let a = &snapshot.partitions[a];
            let b = &snapshot.partitions[b];
            let order = (&a.name, a.lun, a.entry).cmp(&(&b.name, b.lun, b.entry));
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
    }

    fn selection(&self, all: bool) -> Result<PartitionSelection, ActionError> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or(ActionError::Input("请先检查设备分区"))?;
        let partitions = if all {
            if snapshot.luns.is_empty() {
                return Err(ActionError::Input("检查结果没有可用 LUN"));
            }
            PartitionTargets::All
        } else {
            let selected: Vec<_> = snapshot
                .partitions
                .iter()
                .filter(|key| self.selected.contains(*key))
                .map(|key| PartitionSelector {
                    lun: Some(key.lun),
                    name: key.name.clone(),
                })
                .collect();
            if selected.is_empty() {
                return Err(ActionError::Input("请选择分区"));
            }
            PartitionTargets::Selected(selected)
        };
        Ok(PartitionSelection {
            luns: snapshot.luns.clone(),
            partitions,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Discover(Backend),
    Connect,
    Inspect,
    Run(Mode),
    PreparePackage,
    CloudFetch,
    ReviewIdentity,
    CreateIdentity,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Discover(_) => "发现设备",
            Self::Connect => "连接设备",
            Self::Inspect => "检查分区",
            Self::Run(mode) => mode.label(),
            Self::PreparePackage => "离线准备",
            Self::CloudFetch => "获取云端资产",
            Self::ReviewIdentity => "检查身份路径",
            Self::CreateIdentity => "创建安装身份",
        }
    }

    fn inputs_changed(self, before: &Inputs, after: &Inputs) -> bool {
        match self {
            Self::Discover(backend) => backend != after.discovery_backend(),
            Self::Connect => before.connection() != after.connection(),
            Self::Inspect => before.lun_scope != after.lun_scope,
            Self::Run(Mode::Backup) => {
                before.backup != after.backup || before.lun_scope != after.lun_scope
            }
            Self::Run(Mode::Restore) => {
                before.restore != after.restore
                    || before.verify != after.verify
                    || before.lun_scope != after.lun_scope
            }
            Self::Run(Mode::Flash) => before.flash != after.flash || before.verify != after.verify,
            Self::PreparePackage => {
                before.flash.package != after.flash.package
                    || before.flash.nv_id != after.flash.nv_id
                    || before.flash.config != after.flash.config
                    || before.flash.generated != after.flash.generated
            }
            Self::CloudFetch | Self::ReviewIdentity | Self::CreateIdentity => {
                before.cloud != after.cloud
            }
        }
    }
}

struct Active {
    handle: JobHandle,
    action: Action,
    invalidated: bool,
    decided: bool,
    summary: Option<PreparedSummary>,
    status: Option<JobStatus>,
    dropped: u64,
}

impl Active {
    fn invalidate(&mut self) {
        self.invalidated = true;
        self.summary = None;
        if let Some(summary) = self.handle.prepared_summary()
            && self.handle.decide(summary.id, Decision::Decline).is_ok()
        {
            self.decided = true;
        } else {
            self.handle.request_stop();
        }
    }
}

pub(crate) struct Connected {
    pub(crate) id: SessionId,
    pub(crate) target: Target,
    pub(crate) sector_size: usize,
}

pub struct DesktopApp {
    service: CoreService,
    inputs: Inputs,
    pub(crate) mode: Mode,
    device: Option<Active>,
    offline: Option<Active>,
    pub(crate) connected: Option<Connected>,
    pub(crate) disconnecting: bool,
    pub(crate) closing: bool,
    pub(crate) partitions: Partitions,
    pub(crate) candidates: Vec<DeviceCandidate>,
    results: [Option<Arc<OperationResult>>; 2],
    pub(crate) diagnostic_export: Option<diagnostics::PendingExport>,
    pub(crate) result_slot: Slot,
    pub(crate) pending_identity: Option<PathBuf>,
    pub(crate) notice: Option<String>,
    pub(crate) logs: VecDeque<Event>,
    log_bytes: usize,
    pub(crate) dropped_logs: u64,
    sampled_at: Option<Instant>,
    pub(crate) compact_panel: Option<SidePanel>,
    persistence: bool,
}

impl DesktopApp {
    /// The native app and offline tests drive the same state transitions.
    pub fn with_service(service: CoreService) -> Self {
        Self {
            service,
            inputs: Inputs::default(),
            mode: Mode::default(),
            device: None,
            offline: None,
            connected: None,
            disconnecting: false,
            closing: false,
            partitions: Partitions::default(),
            candidates: Vec::new(),
            results: [None, None],
            diagnostic_export: None,
            result_slot: Slot::Device,
            pending_identity: None,
            notice: None,
            logs: VecDeque::new(),
            log_bytes: 0,
            dropped_logs: 0,
            sampled_at: None,
            compact_panel: None,
            persistence: false,
        }
    }

    pub fn native(ctx: &egui::Context, preferences: Preferences) -> Self {
        let mut app = Self::with_ui_service(ctx, CoreService::new(HostServices::native()));
        preferences.apply(&mut app);
        app.persistence = true;
        app
    }

    pub fn with_ui_service(ctx: &egui::Context, service: CoreService) -> Self {
        let app = Self::with_service(service);
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        views::configure_style(ctx);
        let mut fonts = egui::FontDefinitions::default();
        let font = egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansCJKsc-Regular.otf"
        ));
        fonts.font_data.insert("noto_cjk_sc".into(), font.into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("noto_cjk_sc".into());
        }
        ctx.set_fonts(fonts);
        app
    }

    pub fn inputs(&self) -> &Inputs {
        &self.inputs
    }

    pub fn update_inputs(&mut self, next: Inputs) {
        if self.closing || self.inputs == next {
            return;
        }
        let connection_changed = self.inputs.connection() != next.connection();
        if self.inputs.discovery_backend() != next.discovery_backend() {
            self.candidates.clear();
        }
        if self.inputs.lun_scope != next.lun_scope {
            self.partitions.replace(None);
        }
        if self.inputs.cloud != next.cloud {
            self.pending_identity = None;
        }
        for active in [&mut self.device, &mut self.offline].into_iter().flatten() {
            if active.action.inputs_changed(&self.inputs, &next) {
                active.invalidate();
            }
        }
        self.inputs = next;
        if connection_changed
            && (self.connected.is_some()
                || self
                    .device
                    .as_ref()
                    .is_some_and(|active| active.action == Action::Connect))
        {
            self.disconnect();
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if !self.closing && self.pending_write().is_none() {
            self.mode = mode;
            self.compact_panel = None;
        }
    }

    pub fn session(&self) -> Option<SessionId> {
        self.connected.as_ref().map(|device| device.id)
    }

    pub fn snapshot(&self) -> Option<&PartitionSnapshot> {
        self.partitions.snapshot.as_ref()
    }

    pub fn candidates(&self) -> &[DeviceCandidate] {
        &self.candidates
    }

    pub fn recent_events(&self) -> impl Iterator<Item = &Event> {
        self.logs.iter()
    }

    pub fn selected(&self) -> &HashSet<PartitionKey> {
        &self.partitions.selected
    }

    pub fn filter_partitions(&mut self, filter: String, descending: bool) {
        self.partitions.filter = filter;
        self.partitions.descending = descending;
        self.partitions.refresh_view();
    }

    pub fn visible_partitions(&self) -> impl Iterator<Item = &PartitionKey> {
        self.partitions
            .visible
            .iter()
            .filter_map(|&index| self.snapshot()?.partitions.get(index))
    }

    pub fn select_partition(&mut self, key: &PartitionKey, selected: bool) -> bool {
        if !self
            .snapshot()
            .is_some_and(|snapshot| snapshot.partitions.contains(key))
        {
            return false;
        }
        let changed = if selected {
            self.partitions.selected.insert(key.clone())
        } else {
            self.partitions.selected.remove(key)
        };
        if changed {
            self.invalidate_selection_job();
        }
        changed
    }

    pub fn select_all_partitions(&mut self) {
        if let Some(snapshot) = &self.partitions.snapshot
            && self.partitions.selected.len() != snapshot.partitions.len()
        {
            self.partitions
                .selected
                .extend(snapshot.partitions.iter().cloned());
            self.invalidate_selection_job();
        }
    }

    pub fn clear_partitions(&mut self) {
        self.partitions.retained_selection.clear();
        if !self.partitions.selected.is_empty() {
            self.partitions.selected.clear();
            self.invalidate_selection_job();
        }
    }

    fn invalidate_selection_job(&mut self) {
        if let Some(active) = &mut self.device
            && matches!(active.action, Action::Run(Mode::Backup | Mode::Restore))
        {
            active.invalidate();
        }
    }

    pub fn discover(&mut self) -> Result<u64, ActionError> {
        let backend = self.inputs.discovery_backend();
        self.submit(
            Slot::Device,
            Action::Discover(backend),
            Request::Discover { backend },
            None,
        )
    }

    pub fn connect(&mut self) -> Result<u64, ActionError> {
        if self.connected.is_some() {
            return Err(ActionError::Input("请先断开当前设备"));
        }
        let request = Request::Connect(self.inputs.connection()?);
        self.submit(Slot::Device, Action::Connect, request, None)
    }

    pub fn inspect(&mut self) -> Result<u64, ActionError> {
        let id = self.live_session()?;
        let scope = self.inputs.scope()?;
        self.submit(
            Slot::Device,
            Action::Inspect,
            Request::Inspect(id, scope),
            None,
        )
    }

    fn live_session(&self) -> Result<SessionId, ActionError> {
        self.session()
            .filter(|&id| self.service.session_is_current(id))
            .ok_or(ActionError::Input("请先连接设备"))
    }

    fn current_selection(&self) -> Result<(OperationRequest, Option<u64>), ActionError> {
        let inputs = &self.inputs;
        let operation = match self.mode {
            Mode::Backup => {
                require_path(&inputs.backup.output, "请选择备份输出目录")?;
                OperationRequest::Backup {
                    output: inputs.backup.output.clone(),
                    selection: self.partitions.selection(false)?,
                    overwrite: inputs.backup.overwrite,
                }
            }
            Mode::Restore => {
                let all = inputs.restore.kind == RestoreKind::Directory && inputs.restore.all;
                let selection = self.partitions.selection(all)?;
                let (from, image) = match inputs.restore.kind {
                    RestoreKind::Image => {
                        require_path(&inputs.restore.image, "请选择恢复镜像")?;
                        if selection.partitions.single_target().is_none() {
                            return Err(ActionError::Input("单镜像恢复需要且只允许一个目标分区"));
                        }
                        (None, Some(inputs.restore.image.clone()))
                    }
                    RestoreKind::Directory => {
                        require_path(&inputs.restore.directory, "请选择恢复目录")?;
                        (Some(inputs.restore.directory.clone()), None)
                    }
                };
                OperationRequest::Restore {
                    from,
                    image,
                    selection,
                    verify: inputs.verify,
                }
            }
            Mode::Flash => {
                return Ok((
                    OperationRequest::Flash {
                        input: inputs.flash.flash_input()?,
                        verify: inputs.verify,
                        lun: None,
                        set_bootable: inputs.flash.boot_lun,
                        reboot: inputs.flash.reboot,
                    },
                    None,
                ));
            }
        };
        Ok((operation, self.snapshot().map(|snapshot| snapshot.revision)))
    }

    pub(crate) fn blocked_reason(&self) -> Option<ActionError> {
        if self.busy(Slot::Device) || self.disconnecting || self.closing {
            return Some(SubmitError::Busy.into());
        }
        self.live_session()
            .and_then(|_| self.current_selection())
            .err()
    }

    pub fn start(&mut self) -> Result<u64, ActionError> {
        let session = self.live_session()?;
        let (operation, revision) = self.current_selection()?;
        self.submit(
            Slot::Device,
            Action::Run(self.mode),
            Request::Run {
                connection: Connection::Existing(session),
                operation,
            },
            revision.map(|revision| (session, revision)),
        )
    }

    pub fn prepare_package(&mut self) -> Result<u64, ActionError> {
        let input = self.inputs.flash.package_input()?;
        let output = self.inputs.flash.generated.clone();
        require_path(&output, "请输入新的准备目录")?;
        self.submit_offline(
            Action::PreparePackage,
            OperationRequest::PreparePackage { input, output },
        )
    }

    pub fn fetch_cloud(&mut self) -> Result<u64, ActionError> {
        let input = self.inputs.cloud.request(true)?;
        self.submit_offline(Action::CloudFetch, OperationRequest::CloudFetch(input))
    }

    pub fn review_identity(&mut self) -> Result<u64, ActionError> {
        let input = self.inputs.cloud.request(false)?;
        self.submit_offline(
            Action::ReviewIdentity,
            OperationRequest::CloudIdentity {
                input,
                confirmed_path: None,
            },
        )
    }

    pub fn confirm_identity(&mut self, create: bool) -> Result<(), ActionError> {
        let path = self
            .pending_identity
            .take()
            .ok_or(SubmitError::InvalidRequest)?;
        if create {
            let input = self.inputs.cloud.request(false)?;
            self.submit_offline(
                Action::CreateIdentity,
                OperationRequest::CloudIdentity {
                    input,
                    confirmed_path: Some(path),
                },
            )?;
        }
        Ok(())
    }

    pub(crate) fn use_cloud_assets(&mut self) {
        let Some(ResultData::Assets { assets }) = self.offline_data() else {
            return;
        };
        let mut next = self.inputs.clone();
        next.loader = assets.loader.clone();
        next.signed_digest = Some(assets.digest.clone());
        next.digest_signature = Some(assets.signature.clone());
        next.use_cloud = false;
        self.update_inputs(next);
    }

    fn submit_offline(
        &mut self,
        action: Action,
        operation: OperationRequest,
    ) -> Result<u64, ActionError> {
        self.submit(
            Slot::Offline,
            action,
            Request::Run {
                connection: Connection::Offline,
                operation,
            },
            None,
        )
    }

    fn submit(
        &mut self,
        slot: Slot,
        action: Action,
        request: Request,
        snapshot: Option<(SessionId, u64)>,
    ) -> Result<u64, ActionError> {
        if self.closing || self.busy(slot) || (slot == Slot::Device && self.disconnecting) {
            return Err(SubmitError::Busy.into());
        }
        let handle = if let Some((generation, revision)) = snapshot {
            self.service
                .submit_for_snapshot(request, generation, revision)?
        } else {
            self.service.submit(request)?
        };
        let id = handle.operation_id();
        let status = handle.status();
        *self.active_mut(slot) = Some(Active {
            handle,
            action,
            status,
            invalidated: false,
            decided: false,
            summary: None,
            dropped: 0,
        });
        self.notice = None;
        Ok(id)
    }

    fn active(&self, slot: Slot) -> &Option<Active> {
        match slot {
            Slot::Device => &self.device,
            Slot::Offline => &self.offline,
        }
    }

    fn active_mut(&mut self, slot: Slot) -> &mut Option<Active> {
        match slot {
            Slot::Device => &mut self.device,
            Slot::Offline => &mut self.offline,
        }
    }

    pub fn busy(&self, slot: Slot) -> bool {
        self.active(slot).is_some()
    }

    pub fn pending_write(&self) -> Option<&PreparedSummary> {
        self.device.as_ref()?.summary.as_ref()
    }

    pub fn decide(&mut self, id: PreparationId, decision: Decision) -> Result<(), ActionError> {
        let active = self.device.as_mut().ok_or(SubmitError::InvalidRequest)?;
        let summary = active
            .summary
            .as_ref()
            .filter(|summary| summary.id == id)
            .ok_or(SubmitError::InvalidRequest)?;
        if self.closing
            || self.disconnecting
            || active.invalidated
            || !self.service.session_is_current(summary.session_generation)
            || summary.snapshot_revision.is_some_and(|revision| {
                !self
                    .service
                    .snapshot_is_current(summary.session_generation, revision)
            })
        {
            active.invalidate();
            return Err(SubmitError::StaleSession.into());
        }
        active.handle.decide(id, decision)?;
        active.decided = true;
        active.summary = None;
        Ok(())
    }

    pub fn stop(&mut self, slot: Slot) {
        if let Some(active) = self.active_mut(slot) {
            active.summary = None;
            active.decided = true;
            active.handle.request_stop();
            active.status = active.handle.status();
        }
    }

    pub fn disconnect(&mut self) {
        if self.disconnecting {
            return;
        }
        self.connected = None;
        self.partitions.replace(None);
        self.disconnecting = true;
        if let Some(active) = &mut self.device {
            active.summary = None;
            active.decided = true;
        }
        self.service.request_close(StopReason::User);
    }

    pub fn request_window_close(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.pending_identity = None;
        if let Some(active) = &mut self.device {
            active.summary = None;
            active.decided = true;
        }
        self.service.request_close(StopReason::WindowClose);
        if let Some(active) = &self.offline {
            active
                .handle
                .request_stop_with_reason(StopReason::WindowClose);
        }
    }

    pub fn close_ready(&self) -> bool {
        self.device.is_none()
            && self.offline.is_none()
            && self.diagnostic_export.is_none()
            && self.service.device_released()
    }

    pub fn result(&self, slot: Slot) -> Option<&OperationResult> {
        self.results[usize::from(slot == Slot::Offline)].as_deref()
    }

    pub(crate) fn offline_data(&self) -> Option<&ResultData> {
        self.result(Slot::Offline)
            .filter(|result| result.status == Status::Success)?
            .data
            .as_ref()
    }

    pub(crate) fn stop_requested(&self, slot: Slot) -> bool {
        match self.active(slot) {
            Some(active) => active
                .status
                .is_some_and(|status| status.stop_reason.is_some()),
            None => self
                .result(slot)
                .is_some_and(|result| result.stop_reason.is_some()),
        }
    }

    pub(crate) fn export_diagnostics(&mut self, destination: PathBuf) -> io::Result<()> {
        if self.diagnostic_export.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "diagnostics export in progress",
            ));
        }
        let snapshot = diagnostics::Snapshot::capture(
            &self.logs,
            self.dropped_logs,
            self.results[usize::from(self.result_slot == Slot::Offline)].clone(),
        );
        self.diagnostic_export = Some(diagnostics::PendingExport::spawn(
            destination,
            move |path| snapshot.write(path),
        )?);
        self.notice = Some("正在导出诊断".into());
        Ok(())
    }

    pub(crate) fn remember_error<T>(&mut self, result: Result<T, ActionError>) {
        if let Err(error) = result {
            self.notice = Some(error.to_string());
        }
    }

    pub(crate) fn current_job(&self, slot: Slot) -> Option<(&'static str, JobStatus)> {
        let active = self.active(slot).as_ref()?;
        Some((active.action.label(), active.status?))
    }

    /// Poll bounded logs and collect terminal results without blocking the GUI.
    pub fn tick(&mut self) {
        if let Some(notice) = self
            .diagnostic_export
            .as_ref()
            .and_then(diagnostics::PendingExport::poll)
        {
            self.diagnostic_export = None;
            self.notice = Some(notice);
        }
        let sample = self
            .sampled_at
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(100));
        if sample {
            self.sampled_at = Some(Instant::now());
        }
        for slot in [Slot::Device, Slot::Offline] {
            let Some(mut active) = self.active_mut(slot).take() else {
                continue;
            };
            let status = active.handle.status();
            if let Some(status) = status {
                self.dropped_logs += status.dropped_logs.saturating_sub(active.dropped);
                active.dropped = status.dropped_logs;
            }
            if sample || active.status.is_none() {
                active.status = status;
            }
            for _ in 0..256 {
                let Some(event) = active.handle.poll_event() else {
                    break;
                };
                if !active.invalidated
                    && event.operation_id == Some(active.handle.operation_id())
                    && event.session_generation.is_none_or(|generation| {
                        status.is_some_and(|status| status.session_generation == Some(generation))
                    })
                {
                    self.log_bytes += event.message.len();
                    self.logs.push_back(event);
                    while self.logs.len() > 2000 || self.log_bytes > 2 * 1024 * 1024 {
                        self.log_bytes -= self.logs.pop_front().unwrap().message.len();
                        self.dropped_logs += 1;
                    }
                }
            }
            if let Some(result) = active.handle.try_take_result() {
                self.collect(slot, active, result);
            } else {
                if !self.closing
                    && !self.disconnecting
                    && !active.invalidated
                    && !active.decided
                    && active.summary.is_none()
                    && let Some(summary) = active.handle.prepared_summary()
                    && summary.operation_id == active.handle.operation_id()
                    && self.service.session_is_current(summary.session_generation)
                {
                    active.summary = Some(summary);
                }
                *self.active_mut(slot) = Some(active);
            }
        }
        if self.disconnecting && self.service.device_released() {
            self.disconnecting = false;
        }
        if self
            .session()
            .is_some_and(|session| !self.service.session_is_current(session))
        {
            self.connected = None;
            self.partitions.replace(None);
        }
        if self.snapshot().is_some_and(|snapshot| {
            !self
                .service
                .snapshot_is_current(snapshot.generation, snapshot.revision)
        }) {
            self.partitions.invalidate();
        }
    }

    fn collect(&mut self, slot: Slot, active: Active, result: OperationResult) {
        if !active.invalidated && !self.closing && result.status == Status::Success {
            if active.action == Action::Connect
                && result.session.disposition == SessionDisposition::Reusable
                && let (Some(id), Some(target)) =
                    (result.session.generation, result.target.as_ref())
                && self.service.session_is_current(id)
            {
                let sector_size = self.inputs.sector_size.unwrap_or(match self.inputs.memory {
                    Memory::Emmc => 512,
                    Memory::Ufs => 4096,
                });
                self.connected = Some(Connected {
                    id,
                    target: target.clone(),
                    sector_size,
                });
            }
            match &result.data {
                Some(ResultData::Candidates { candidates })
                    if matches!(active.action, Action::Discover(backend)
                    if backend == self.inputs.discovery_backend()) =>
                {
                    self.candidates.clone_from(candidates)
                }
                Some(ResultData::Snapshot { snapshot })
                    if self.session() == Some(snapshot.generation)
                        && self
                            .service
                            .snapshot_is_current(snapshot.generation, snapshot.revision) =>
                {
                    self.partitions.replace(Some(snapshot.clone()));
                }
                Some(ResultData::Identity {
                    identity_path,
                    created: false,
                }) if active.action == Action::ReviewIdentity => {
                    self.pending_identity = Some(identity_path.clone())
                }
                _ => {}
            }
        }
        self.results[usize::from(slot == Slot::Offline)] = Some(Arc::new(result));
        self.result_slot = slot;
    }

    pub(crate) fn clear_log(&mut self) {
        self.logs.clear();
        self.log_bytes = 0;
        self.dropped_logs = 0;
    }
}

impl eframe::App for DesktopApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick();
        if ctx.input(|input| input.viewport().close_requested()) && !self.close_ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_window_close();
        }
        if self.closing && self.close_ready() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.closing
            || self.disconnecting
            || self.device.is_some()
            || self.offline.is_some()
            || self.diagnostic_export.is_some()
        {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        views::workspace(self, ui, frame);
        if let Some(summary) = self.pending_write()
            && let Some(decision) = dialogs::confirm_write(ui.ctx(), summary)
        {
            let result = self.decide(summary.id, decision);
            self.remember_error(result);
        }
        if let Some(path) = &self.pending_identity
            && let Some(create) = dialogs::confirm_identity(ui.ctx(), path)
        {
            let result = self.confirm_identity(create);
            self.remember_error(result);
        }
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        if self.persistence {
            let _ = Preferences::capture(self).save();
        }
    }
}

#[cfg(test)]
#[path = "../../oplus-edl-core/tests/support/mod.rs"]
mod protocol_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread};

    #[test]
    fn connection_events_keep_device_log_text_out_of_the_gui() {
        use super::protocol_fixture::*;
        let root = tempfile::tempdir().unwrap();
        let mut script = sahara_script();
        script.extend([
            response(b"<data><log value=\"private fixture token\"/></data>"),
            response(WELCOME_END),
            response(CONFIG_ACK),
        ]);
        let (host, trace) = scripted_host(script);
        let mut app = DesktopApp::with_service(CoreService::new(host));
        let mut inputs = app.inputs().clone();
        inputs.loader = root.path().join("loader.elf");
        std::fs::write(&inputs.loader, b"abcd").unwrap();
        inputs.device = Some("scripted".into());
        inputs.memory = Memory::Emmc;
        app.update_inputs(inputs);
        app.connect().unwrap();
        let result = wait_for(|| {
            app.tick();
            app.result(Slot::Device).cloned()
        });
        assert_eq!(result.status, Status::Success);
        assert!(
            app.logs
                .iter()
                .any(|event| event.message == "Firehose: startup log received")
        );
        assert!(
            app.logs
                .iter()
                .all(|event| !event.message.contains("private fixture token"))
        );
        assert_eq!(wire_commands(&trace)[0].name, "configure");
    }

    #[test]
    fn desktop_uses_native_display_scale_without_keyboard_zoom() {
        let ctx = egui::Context::default();
        let _app = DesktopApp::with_ui_service(&ctx, CoreService::new(HostServices::default()));
        let mut input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Equals,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(2.0);
        ctx.run_ui(input.clone(), |_| {}).textures_delta.clear();
        input.events.clear();
        ctx.run_ui(input, |_| {}).textures_delta.clear();
        assert_eq!(ctx.zoom_factor(), 1.0);
        assert_eq!(ctx.pixels_per_point(), 2.0);
    }

    #[test]
    fn stop_feedback_survives_collection_before_the_next_frame() {
        let (entered, device_running) = mpsc::channel();
        let (release, pending) = mpsc::channel();
        let pending = std::sync::Mutex::new(pending);
        let limit = Duration::from_secs(3);
        let host = HostServices::default().with_backend(
            Backend::Usb,
            move || {
                entered.send(()).unwrap();
                pending.lock().unwrap().recv_timeout(limit).unwrap();
                Ok(vec![])
            },
            |_, _| panic!("discovery must not open a device"),
        );
        let mut app = DesktopApp::with_service(CoreService::new(host));
        app.discover().unwrap();
        device_running.recv_timeout(limit).unwrap();
        assert!(!app.stop_requested(Slot::Device));
        app.stop(Slot::Device);
        assert!(app.stop_requested(Slot::Device));
        release.send(()).unwrap();
        let started = Instant::now();
        while app.result(Slot::Device).is_none() {
            app.tick();
            assert!(started.elapsed() < limit);
            thread::yield_now();
        }
        assert!(app.current_job(Slot::Device).is_none());
        assert!(app.stop_requested(Slot::Device));
        assert_eq!(app.result(Slot::Device).unwrap().status, Status::Stopped);
        assert!(!app.stop_requested(Slot::Offline));
    }

    #[test]
    fn delayed_diagnostic_export_does_not_hold_up_stop_or_window_close_requests() {
        let (entered, device_running) = mpsc::channel();
        let (release_device, pending_device) = mpsc::channel();
        let pending_device = std::sync::Mutex::new(pending_device);
        let limit = Duration::from_secs(3);
        let host = HostServices::default().with_backend(
            Backend::Usb,
            move || {
                entered.send(()).unwrap();
                pending_device.lock().unwrap().recv_timeout(limit).unwrap();
                Ok(vec![])
            },
            |_, _| panic!("discovery must not open a device"),
        );
        let mut app = DesktopApp::with_service(CoreService::new(host));
        let (release_export, pending_export) = mpsc::channel();
        app.diagnostic_export = Some(
            diagnostics::PendingExport::spawn("unused.json".into(), move |_| {
                pending_export.recv_timeout(limit).unwrap();
                Ok(())
            })
            .unwrap(),
        );
        app.discover().unwrap();
        device_running.recv_timeout(limit).unwrap();
        app.stop(Slot::Device);
        app.request_window_close();
        assert!(app.closing && !app.close_ready());
        release_device.send(()).unwrap();
        let started = Instant::now();
        while app.result(Slot::Device).is_none() {
            app.tick();
            assert!(started.elapsed() < limit);
            thread::sleep(Duration::from_millis(1));
        }
        let result = app.result(Slot::Device).unwrap();
        assert_eq!(result.status, Status::Stopped);
        assert_eq!(result.stop_reason, Some(StopReason::User));
        assert!(!app.close_ready());
        release_export.send(()).unwrap();
        let started = Instant::now();
        while !app.close_ready() {
            app.tick();
            assert!(started.elapsed() < limit);
            thread::sleep(Duration::from_millis(1));
        }
    }
}
