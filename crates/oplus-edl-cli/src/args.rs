use crate::render::OutputMode;
use clap::{
    Args, CommandFactory, Parser, Subcommand,
    builder::{PossibleValuesParser, TypedValueParser},
};
use oplus_edl_core::model::PartitionSelector;
pub use oplus_edl_core::model::{Backend, Memory, RawSize, Verify};
use std::{ffi::OsString, path::PathBuf};

fn backend_parser() -> impl TypedValueParser<Value = Backend> {
    PossibleValuesParser::new(["usb", "serial"]).map(|value| match value.as_str() {
        "usb" => Backend::Usb,
        _ => Backend::Serial,
    })
}

fn memory_parser() -> impl TypedValueParser<Value = Memory> {
    PossibleValuesParser::new(["emmc", "ufs"]).map(|value| match value.as_str() {
        "emmc" => Memory::Emmc,
        _ => Memory::Ufs,
    })
}

fn sector_parser() -> impl TypedValueParser<Value = usize> {
    PossibleValuesParser::new(["512", "4096"]).map(|value| match value.as_str() {
        "512" => 512,
        _ => 4096,
    })
}

fn raw_parser() -> impl TypedValueParser<Value = RawSize> {
    PossibleValuesParser::new(["declared", "file"]).map(|value| match value.as_str() {
        "declared" => RawSize::Declared,
        _ => RawSize::File,
    })
}

fn verification_parser() -> impl TypedValueParser<Value = Verify> {
    PossibleValuesParser::new(["best-effort", "required", "none"]).map(|value| {
        match value.as_str() {
            "required" => Verify::Required,
            "none" => Verify::None,
            _ => Verify::BestEffort,
        }
    })
}

fn partition_parser(input: &str) -> Result<PartitionSelector, &'static str> {
    let (lun, name) = input.split_once(':').ok_or("expected LUN:NAME")?;
    if lun.is_empty() || !lun.bytes().all(|b| b.is_ascii_digit()) || name.is_empty() {
        return Err("invalid LUN:NAME");
    }
    Ok(PartitionSelector {
        lun: Some(lun.parse().map_err(|_| "LUN must be between 0 and 255")?),
        name: name.into(),
    })
}

#[derive(Debug, Parser)]
#[command(
    name = "oplus-edl",
    version,
    propagate_version = true,
    about = "Oplus EDL — inspect, back up, restore and flash devices"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct ConnectionArgs {
    #[arg(long, value_parser = backend_parser(), default_value = "usb")]
    pub backend: Backend,
    #[arg(long)]
    pub device: Option<String>,
    #[arg(long, value_parser = memory_parser(), default_value = "ufs")]
    pub memory: Memory,
    #[arg(long, value_parser = sector_parser())]
    pub sector_size: Option<usize>,
    /// Omit for one discovery attempt. Zero waits until stopped.
    #[arg(long, value_name = "SECONDS")]
    pub wait_timeout: Option<u64>,
    #[arg(long, value_name = "FILE")]
    pub loader: Option<PathBuf>,
    #[arg(long, value_name = "FILE", requires = "loader")]
    pub signed_digest: Option<PathBuf>,
    #[arg(long, value_name = "FILE", requires = "signed_digest")]
    pub digest_signature: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct FlashConnection {
    #[command(flatten)]
    pub local: ConnectionArgs,
    #[arg(
        long,
        value_name = "FILE",
        requires = "cloud_output",
        conflicts_with_all = ["loader", "signed_digest", "digest_signature"]
    )]
    pub cloud_config: Option<PathBuf>,
    #[arg(long, value_name = "DIR", requires = "cloud_config")]
    pub cloud_output: Option<PathBuf>,
    /// Create a new SWM identity at the configured path.
    #[arg(long, requires = "cloud_config")]
    pub create_identity: bool,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SelectionArgs {
    #[arg(long, value_name = "LUN:NAME", value_parser = partition_parser)]
    pub partition: Vec<PartitionSelector>,
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct WriteArgs {
    #[arg(long)]
    pub yes: bool,
    #[arg(long, value_parser = verification_parser(), default_value = "best-effort")]
    pub verify: Verify,
}

#[derive(Debug, Args)]
pub struct FlashArgs {
    #[command(flatten)]
    pub connection: FlashConnection,
    #[command(flatten)]
    pub write: WriteArgs,
    #[arg(long, value_name = "LUN")]
    pub set_bootable: Option<u8>,
    #[arg(long)]
    pub reboot: bool,
}

#[derive(Debug, Args)]
pub struct PackageArgs {
    #[arg(long, value_name = "DIR")]
    pub package: PathBuf,
    /// Digit string from META/super_def.<ID>.json.
    #[arg(long, value_name = "ID")]
    pub nv_id: String,
    #[arg(long, value_name = "FILE")]
    pub flash_config: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum RestoreCommand {
    Image {
        #[command(flatten)]
        connection: ConnectionArgs,
        #[arg(long, value_name = "FILE")]
        image: PathBuf,
        #[arg(long, value_name = "LUN:NAME", value_parser = partition_parser)]
        partition: PartitionSelector,
        #[command(flatten)]
        write: WriteArgs,
    },
    Directory {
        #[command(flatten)]
        connection: ConnectionArgs,
        #[arg(long, value_name = "DIR")]
        from: PathBuf,
        #[command(flatten)]
        selection: SelectionArgs,
        #[arg(long, value_name = "N")]
        lun: Vec<u8>,
        #[command(flatten)]
        write: WriteArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum FlashCommand {
    Xml {
        #[command(flatten)]
        common: FlashArgs,
        #[arg(long, value_name = "FILE", required = true)]
        program: Vec<PathBuf>,
        #[arg(long, value_name = "FILE")]
        patch: Vec<PathBuf>,
        #[arg(long, value_parser = raw_parser(), default_value = "declared")]
        raw_size: RawSize,
    },
    Package {
        #[command(flatten)]
        common: FlashArgs,
        #[command(flatten)]
        package: PackageArgs,
        #[arg(
            long,
            value_name = "DIR",
            required_unless_present = "prepared",
            conflicts_with = "prepared"
        )]
        prepare_output: Option<PathBuf>,
        #[arg(long, value_name = "DIR")]
        prepared: Option<PathBuf>,
        /// Package operations use the sector-rounded file size capped by XML.
        #[arg(long, value_parser = ["file"], default_value = "file")]
        raw_size: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PackageCommand {
    Prepare {
        #[command(flatten)]
        package: PackageArgs,
        #[arg(long, value_name = "DIR")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum CloudCommand {
    Fetch {
        #[arg(long, value_name = "FILE")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        output: PathBuf,
        /// Create a new SWM identity at the configured path.
        #[arg(long)]
        create_identity: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List candidates on one transport.
    Devices {
        #[arg(long, value_parser = backend_parser(), default_value = "usb")]
        backend: Backend,
    },
    /// Initialize a device and read the selected GPTs.
    Partitions {
        #[command(flatten)]
        connection: ConnectionArgs,
        #[arg(long, value_name = "N")]
        lun: Vec<u8>,
    },
    /// Read explicitly selected partitions or all partitions in scope.
    Backup {
        #[command(flatten)]
        connection: ConnectionArgs,
        #[arg(long, value_name = "DIR")]
        output: PathBuf,
        #[command(flatten)]
        selection: SelectionArgs,
        #[arg(long, value_name = "N")]
        lun: Vec<u8>,
        #[arg(long)]
        overwrite: bool,
    },
    #[command(subcommand)]
    Restore(RestoreCommand),
    #[command(subcommand)]
    Flash(FlashCommand),
    #[command(subcommand)]
    Package(PackageCommand),
    #[command(subcommand)]
    Cloud(CloudCommand),
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Devices { .. } => "devices",
            Self::Partitions { .. } => "partitions",
            Self::Backup { .. } => "backup",
            Self::Restore(_) => "restore",
            Self::Flash(_) => "flash",
            Self::Package(_) => "package_prepare",
            Self::Cloud(_) => "cloud_fetch",
        }
    }

    pub fn verify(&self) -> Option<&'static str> {
        match self {
            Self::Restore(
                RestoreCommand::Image { write, .. } | RestoreCommand::Directory { write, .. },
            ) => Some(write.verify.as_str()),
            Self::Flash(
                FlashCommand::Xml { common, .. } | FlashCommand::Package { common, .. },
            ) => Some(common.write.verify.as_str()),
            _ => None,
        }
    }
}

/// Recover output preferences when parsing exits before returning a typed command.
pub fn early_output(args: &[OsString]) -> (OutputMode, Option<&'static str>) {
    let mut root = Cli::command();
    root.build();
    let mut command = &root;
    let mut mode = OutputMode::default();
    let mut operation = None;
    let mut tokens = args.iter().skip(1).peekable();
    while let Some(token) = tokens.next() {
        if token == "--" {
            break;
        }
        let Some(token) = token.to_str() else {
            continue;
        };
        match token {
            "--json" => mode.json = true,
            "--quiet" | "-q" => mode.quiet = true,
            _ => {}
        }
        if let Some(long) = token.strip_prefix("--") {
            if !long.contains('=')
                && let Some(arg) = command
                    .get_arguments()
                    .find(|arg| arg.get_long() == Some(long) && arg.get_action().takes_values())
                && tokens.peek().is_some_and(|next| {
                    *next != "--"
                        && (arg.is_allow_hyphen_values_set()
                            || *next == "-"
                            || !next.as_encoded_bytes().starts_with(b"-"))
                })
            {
                tokens.next();
            }
        } else if let Some(child) = command.find_subcommand(token) {
            if operation.is_none() {
                operation = match token {
                    "devices" => Some("devices"),
                    "partitions" => Some("partitions"),
                    "backup" => Some("backup"),
                    "restore" => Some("restore"),
                    "flash" => Some("flash"),
                    "package" => Some("package_prepare"),
                    "cloud" => Some("cloud_fetch"),
                    _ => None,
                };
            }
            command = child;
        }
    }
    (mode, operation)
}
