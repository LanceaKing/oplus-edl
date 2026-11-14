use crate::cli::{Backend, Cli, Memory};
use anyhow::{Result, bail};
use qdl::types::{
    FirehoseConfiguration, FirehoseStatus, FirehoseStorageType, QdlBackend, QdlChan, QdlDevice,
    QdlReadWrite,
};

pub fn configuration(cli: &Cli) -> Result<FirehoseConfiguration> {
    let sector = cli.sector_size.unwrap_or(match cli.memory {
        Memory::Emmc => 512,
        Memory::Ufs => 4096,
    });
    // These are the GPT sector sizes supported by the first product implementation.
    if !matches!(sector, 512 | 4096) {
        bail!("sector size must be 512 or 4096");
    }
    let config = FirehoseConfiguration {
        storage_sector_size: sector,
        storage_type: match cli.memory {
            Memory::Emmc => FirehoseStorageType::Emmc,
            Memory::Ufs => FirehoseStorageType::Ufs,
        },
        backend: match cli.backend {
            Backend::Usb => QdlBackend::Usb,
            Backend::Serial => QdlBackend::Serial,
        },
        quiet: true,
        ..Default::default()
    };
    config.validate()?;
    Ok(config)
}
pub fn candidates(backend: Backend) -> Result<Vec<String>> {
    match backend {
        Backend::Usb => qdl::usb::list_usb_device_keys(),
        Backend::Serial => qdl::serial::list_serial_device_keys(),
    }
}
pub fn open(
    backend: Backend,
    selected: &str,
    config: FirehoseConfiguration,
) -> Result<QdlDevice<dyn QdlReadWrite>> {
    let rw: Box<dyn QdlReadWrite> = match backend {
        Backend::Usb => Box::new(qdl::usb::setup_usb_device_by_key(Some(selected))?),
        Backend::Serial => Box::new(qdl::serial::setup_serial_device(Some(selected.into()))?),
    };
    Ok(QdlDevice {
        rw,
        fh_cfg: config,
        reset_on_drop: false,
    })
}
/// Uses an already constructed session; tests never enumerate or open hardware.
pub fn initialize(channel: &mut impl QdlChan, images: &mut [Option<Vec<u8>>]) -> Result<()> {
    channel.mut_fh_config().quiet = true;
    channel.fh_config().validate()?;
    // Requires the reviewed loader-only API: removing the local patch before
    // updating the pinned revision must fail to build, not restore unsafe dispatch.
    qdl::sahara::sahara_load_programmer(channel, images)?;
    qdl::firehose_read_welcome(channel)?;
    configure(channel)
}

pub fn configure(channel: &mut impl QdlChan) -> Result<()> {
    qdl::firehose_configure(channel, false)?;
    if qdl::firehose_read(channel, |channel, attrs| {
        for key in ["SECTOR_SIZE_IN_BYTES", "SectorSizeInBytes"] {
            if let Some(value) = attrs.get(key)
                && value.parse::<usize>()? != channel.fh_config().storage_sector_size
            {
                bail!("device sector size conflicts with selected sector size");
            }
        }
        qdl::parsers::firehose_parser_configure_response(channel, attrs)
    })? != FirehoseStatus::Ack
    {
        bail!("Firehose configuration rejected");
    }
    Ok(())
}
