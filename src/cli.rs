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
