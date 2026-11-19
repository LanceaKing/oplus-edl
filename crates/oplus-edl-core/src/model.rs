use serde::Serialize;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Usb,
    Serial,
}
impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Serial => "serial",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Memory {
    Emmc,
    #[default]
    Ufs,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RawSize {
    #[default]
    Declared,
    File,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verify {
    #[default]
    BestEffort,
    Required,
    None,
}
impl Verify {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BestEffort => "best-effort",
            Self::Required => "required",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    User,
    Signal,
    WindowClose,
    ConsumerLost,
    OutputFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InitializationStep {
    SaharaUpload,
    FirehoseWelcome,
    SignedDigestAck,
    DigestSignatureAck,
    Sha256initAck,
    ConfigureAck,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoaderInput {
    Local {
        loader: Option<PathBuf>,
        signed_digest: Option<PathBuf>,
        digest_signature: Option<PathBuf>,
    },
    Cloud(CloudInput),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IdentityMode {
    #[default]
    OpenExisting,
    CreateNew,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudInput {
    pub config: PathBuf,
    pub output: PathBuf,
    pub base_directory: Option<PathBuf>,
    pub identity_mode: IdentityMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    pub backend: Backend,
    pub device: Option<String>,
    pub memory: Memory,
    pub sector_size: Option<usize>,
    pub loader: LoaderInput,
    pub wait_timeout: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct SessionId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Selected(Vec<u8>),
    All,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PartitionKey {
    pub generation: SessionId,
    pub snapshot_revision: u64,
    pub lun: u8,
    pub entry: u32,
    pub name: String,
    pub start_sector: String,
    pub sectors: String,
    pub sector_size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartitionSnapshot {
    pub generation: SessionId,
    pub revision: u64,
    pub scope: Scope,
    pub luns: Vec<u8>,
    pub partitions: Vec<PartitionKey>,
}

#[derive(Clone, Debug)]
pub enum Connection {
    Existing(SessionId),
    Open(SessionConfig),
    Offline,
}

#[derive(Clone, Debug)]
pub enum FlashInput {
    Xml {
        programs: Vec<PathBuf>,
        patches: Vec<PathBuf>,
        raw_size: RawSize,
    },
    Package {
        input: PackageInput,
        artifacts: PackageArtifacts,
    },
}

#[derive(Clone, Debug)]
pub struct PackageInput {
    pub root: PathBuf,
    pub nv_id: String,
    pub config: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum PackageArtifacts {
    Generate(PathBuf),
    Reopen(PathBuf),
}

#[derive(Clone, Debug)]
pub struct PartitionSelection {
    /// An empty list uses the discoverable scope unless every target names a LUN.
    pub luns: Vec<u8>,
    pub partitions: PartitionTargets,
}

#[derive(Clone, Debug)]
pub enum PartitionTargets {
    /// Every eligible partition for backup, or every valid available directory mapping.
    All,
    /// Explicit targets. The service rejects an empty list before provider access.
    Selected(Vec<PartitionSelector>),
}

#[derive(Clone, Debug)]
pub struct PartitionSelector {
    pub lun: Option<u8>,
    pub name: String,
}

impl PartitionSelection {
    pub fn explicit_luns(&self) -> Vec<u8> {
        let selected = self.partitions.explicit();
        let mut luns = if selected.is_empty() || selected.iter().any(|p| p.lun.is_none()) {
            self.luns.clone()
        } else {
            selected.iter().filter_map(|p| p.lun).collect()
        };
        luns.sort_unstable();
        luns.dedup();
        luns
    }
}

impl PartitionTargets {
    pub fn single_target(&self) -> Option<&PartitionSelector> {
        match self.explicit() {
            [target] => Some(target),
            _ => None,
        }
    }

    pub(crate) fn from_names(lun: Option<u8>, names: &[String]) -> Self {
        if names.is_empty() {
            return Self::All;
        }
        Self::Selected(
            names
                .iter()
                .map(|name| PartitionSelector {
                    lun,
                    name: name.clone(),
                })
                .collect(),
        )
    }

    pub(crate) fn explicit(&self) -> &[PartitionSelector] {
        match self {
            Self::All => &[],
            Self::Selected(targets) => targets,
        }
    }

    pub(crate) fn includes(&self, lun: u8, name: &str) -> bool {
        matches!(self, Self::All)
            || self
                .explicit()
                .iter()
                .any(|p| p.name == name && p.lun.is_none_or(|n| n == lun))
    }
}

#[derive(Clone, Debug)]
pub enum OperationRequest {
    Inspect {
        scope: Scope,
    },
    Backup {
        output: PathBuf,
        selection: PartitionSelection,
        overwrite: bool,
    },
    Restore {
        from: Option<PathBuf>,
        image: Option<PathBuf>,
        selection: PartitionSelection,
        verify: Verify,
    },
    Flash {
        input: FlashInput,
        verify: Verify,
        lun: Option<u8>,
        set_bootable: Option<u8>,
        reboot: bool,
    },
    PreparePackage {
        input: PackageInput,
        output: PathBuf,
    },
    CloudFetch(CloudInput),
    CloudIdentity {
        input: CloudInput,
        confirmed_path: Option<PathBuf>,
    },
}

impl OperationRequest {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Inspect { .. } => "partitions",
            Self::Backup { .. } => "backup",
            Self::Restore { .. } => "restore",
            Self::Flash { .. } => "flash",
            Self::PreparePackage { .. } => "package_prepare",
            Self::CloudFetch(_) => "cloud_fetch",
            Self::CloudIdentity { .. } => "cloud_identity",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Request {
    Discover {
        backend: Backend,
    },
    Connect(SessionConfig),
    Inspect(SessionId, Scope),
    Run {
        connection: Connection,
        operation: OperationRequest,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparationId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Decline,
}

#[derive(Clone, Debug)]
pub struct PreparedSummary {
    pub id: PreparationId,
    pub operation_id: u64,
    pub session_generation: SessionId,
    pub verify: Verify,
    pub items: Vec<crate::result::Item>,
    /// A package-origin plan has passed the package's project and range checks.
    pub package: Option<crate::package::Preparation>,
    pub snapshot_revision: Option<u64>,
    pub target: Option<crate::result::Target>,
    pub coverage: crate::result::Coverage,
}
