use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Backend {
    Usb,
    Serial,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Memory {
    Emmc,
    Ufs,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum RawSize {
    #[default]
    Declared,
    File,
}
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
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
#[derive(Debug, Parser)]
#[command(
    version,
    about = "Oplus EDL — backup, restore and flash with explicit verification"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub device: Option<String>,
    #[arg(long, global = true, value_enum, default_value = "usb")]
    pub backend: Backend,
    #[arg(long, global = true)]
    pub loader: Option<PathBuf>,
    /// Cloud query and SWM automatic enrollment settings (JSON).
    #[arg(long, global = true, requires = "cloud_output", conflicts_with_all = ["loader", "signed_digest", "digest_signature"])]
    pub cloud_config: Option<PathBuf>,
    /// New directory for verified cloud loader, digest and signature.
    #[arg(long, global = true, requires = "cloud_config")]
    pub cloud_output: Option<PathBuf>,
    /// Signed partition digest table sent after programmer startup, before configure.
    /// This does not provide a device session authorization signature.
    #[arg(long, global = true)]
    pub signed_digest: Option<PathBuf>,
    /// Matching cloud DIGEST signature; selects the preview DIGEST handshake.
    /// Send the supplied file unchanged. Requires the matching loader and digest.
    #[arg(long, global = true, requires = "signed_digest")]
    pub digest_signature: Option<PathBuf>,
    #[arg(long, global = true, value_enum, default_value = "ufs")]
    pub memory: Memory,
    #[arg(long, global = true)]
    pub lun: Option<u8>,
    #[arg(long, global = true)]
    pub sector_size: Option<usize>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    Unlock,
    Lock,
    Backup {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        partition: Vec<String>,
        #[arg(long)]
        overwrite: bool,
    },
    Restore {
        #[arg(long, conflicts_with = "image", required_unless_present = "image")]
        from: Option<PathBuf>,
        #[arg(long, requires = "partition")]
        image: Option<PathBuf>,
        #[arg(long)]
        partition: Vec<String>,
        #[arg(long, value_enum, default_value = "best-effort")]
        verify: Verify,
        #[arg(long)]
        yes: bool,
    },
    Flash {
        /// Wait for an EDL device before opening the session; 0 waits until Ctrl+C.
        #[arg(long, default_value_t = 0)]
        wait_timeout: u64,
        /// Fetch and validate cloud assets without opening a device or preparing images.
        #[arg(long, requires = "cloud_config", conflicts_with_all = ["prepare_only", "package", "program", "patch"])]
        cloud_fetch_only: bool,
        /// Prepare an SFP package; check device OCDT project before writing and preserve LUN5/OCDT.
        #[arg(long, conflicts_with_all = ["program", "patch"], requires_all = ["nv_id", "prepare_output"])]
        package: Option<PathBuf>,
        /// Explicit package variant, e.g. 10010111. Never inferred from a device name.
        #[arg(long, requires = "package")]
        nv_id: Option<String>,
        /// New directory for generated super.img, XML and selection.json.
        #[arg(long, requires = "package")]
        prepare_output: Option<PathBuf>,
        /// Apply write exclusions; other preview settings cannot disable package project checks.
        #[arg(long, requires = "package")]
        flash_config: Option<PathBuf>,
        /// Build and validate inputs without opening a device.
        #[arg(long, requires = "package", conflicts_with = "cloud_config")]
        prepare_only: bool,
        /// RAW transfer length: XML range, or sector-rounded file size capped by XML.
        #[arg(long, value_enum, default_value = "declared")]
        raw_size: RawSize,
        #[arg(long)]
        program: Vec<PathBuf>,
        #[arg(long)]
        patch: Vec<PathBuf>,
        #[arg(long)]
        set_bootable: Option<u8>,
        #[arg(long)]
        reboot: bool,
        #[arg(long, value_enum, default_value = "best-effort")]
        verify: Verify,
        #[arg(long)]
        yes: bool,
    },
}
impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Unlock => "unlock",
            Self::Lock => "lock",
            Self::Backup { .. } => "backup",
            Self::Restore { .. } => "restore",
            Self::Flash { .. } => "flash",
        }
    }
    pub fn verify(&self) -> Option<&'static str> {
        match self {
            Self::Restore { verify, .. } | Self::Flash { verify, .. } => Some(verify.as_str()),
            _ => None,
        }
    }
}
