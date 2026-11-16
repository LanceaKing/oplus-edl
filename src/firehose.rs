//! Product Firehose commands with checked ACK modes and explicit mutation outcomes.
use anyhow::{Context, Result, bail};
use qdl::types::{FirehoseStatus, MutationFailure, QdlBackend, QdlChan, ReadRejected};
use std::io::{Read, Write};
use xmltree::{Element, XMLNode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WireMode {
    #[default]
    Direct,
    PreviewBackupGpt,
}

/// QCOM 0x180035c60 probe kind 1: five sectors at LBA 31, LUN 0.
/// Select the preview transport mode only after both ACKs and the full read.
pub fn probe_preview_mode(channel: &mut impl QdlChan) -> Result<WireMode> {
    let sector = channel.fh_config().storage_sector_size;
    anyhow::ensure!(
        matches!(sector, 512 | 4096),
        "unsupported preview probe sector size"
    );
    let xml = format!(
        "<?xml version=\"1.0\"?><data><read SECTOR_SIZE_IN_BYTES=\"{sector}\" num_partition_sectors=\"5\" physical_partition_number=\"0\" start_sector=\"31\" filename=\"gpt_backup0.bin\" label=\"BackupGPT\" special_rw_mode=\"oplus_gptbackup\"/></data>"
    );
    qdl::firehose_write(channel, &mut xml.into_bytes())?;
    response::<true>(channel).map_err(|e| anyhow::anyhow!("preview read-mode probe: {e:?}"))?;
    let mut data = vec![0; sector * 5];
    channel
        .read_exact(&mut data)
        .context("preview read-mode probe data")?;
    response::<false>(channel)
        .map_err(|e| anyhow::anyhow!("preview read-mode probe completion: {e:?}"))?;
    Ok(WireMode::PreviewBackupGpt)
}

fn response<const RAW: bool>(channel: &mut impl QdlChan) -> Result<(), MutationFailure> {
    let status = qdl::firehose_read(channel, |ch, attrs| {
        let status = qdl::parsers::firehose_parser_ack_nak(ch, attrs)?;
        if status == FirehoseStatus::Ack
            && let Some(mode) = attrs.get("rawmode")
            && mode != if RAW { "true" } else { "false" }
        {
            bail!("Firehose response has unexpected rawmode");
        }
        Ok(status)
    })
    .map_err(MutationFailure::unknown)?;
    match status {
        FirehoseStatus::Ack => Ok(()),
        FirehoseStatus::Nak => Err(MutationFailure::rejected(anyhow::anyhow!(
            "Firehose response was NAKed"
        ))),
    }
}
pub fn program(
    channel: &mut impl QdlChan,
    input: &mut impl Read,
    label: &str,
    sectors: usize,
    slot: u8,
    lun: u8,
    start: &str,
) -> Result<(), MutationFailure> {
    program_with_mode(
        channel,
        input,
        label,
        sectors,
        slot,
        lun,
        start,
        WireMode::Direct,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn program_with_mode(
    channel: &mut impl QdlChan,
    input: &mut impl Read,
    label: &str,
    sectors: usize,
    slot: u8,
    lun: u8,
    start: &str,
    mode: WireMode,
) -> Result<(), MutationFailure> {
    let cfg = *channel.fh_config();
    cfg.validate().map_err(MutationFailure::not_sent)?;
    let total = sectors
        .checked_mul(cfg.storage_sector_size)
        .context("program size overflow")
        .map_err(MutationFailure::not_sent)?;
    if total == 0 {
        return Err(MutationFailure::not_sent(anyhow::anyhow!("empty program")));
    }
    let mut element = Element::new("program");
    for (key, value) in [
        ("SECTOR_SIZE_IN_BYTES", cfg.storage_sector_size.to_string()),
        ("num_partition_sectors", sectors.to_string()),
        ("physical_partition_number", lun.to_string()),
        ("start_sector", start.into()),
        ("slot", slot.to_string()),
        ("label", label.into()),
        (
            "read_back_verify",
            u8::from(cfg.read_back_verify).to_string(),
        ),
    ] {
        element.attributes.insert(key.into(), value);
    }
    if mode == WireMode::PreviewBackupGpt {
        // Native selection/fallback 0x18005c600 / 0x18003df70. The wire alias
        // does not change the physical range or the report's partition name.
        element
            .attributes
            .insert("label".into(), "BackupGPT".into());
        element
            .attributes
            .insert("filename".into(), format!("gpt_backup{lun}.bin"));
        element
            .attributes
            .insert("special_rw_mode".into(), "oplus_gptbackup".into());
    }
    let mut xml = Element::new("data");
    xml.children.push(XMLNode::Element(element));
    let mut bytes = Vec::new();
    xml.write(&mut bytes).map_err(MutationFailure::not_sent)?;
    qdl::firehose_write(channel, &mut bytes).map_err(MutationFailure::unknown)?;
    response::<true>(channel)?;
    let mut buffer = vec![0; cfg.send_buffer_size.min(total)];
    let mut left = total;
    while left > 0 {
        let size = left.min(buffer.len());
        let block = &mut buffer[..size];
        input
            .read_exact(block)
            .context("program source read failed")
            .map_err(MutationFailure::unknown)?;
        if cfg.backend == QdlBackend::Usb {
            let written = channel.write(block).map_err(MutationFailure::unknown)?;
            if written != size {
                return Err(MutationFailure::unknown(anyhow::anyhow!(
                    "short USB program write"
                )));
            }
        } else {
            channel.write_all(block).map_err(MutationFailure::unknown)?;
        }
        left -= size;
    }
    if cfg.backend == QdlBackend::Usb {
        channel.write(&[]).map_err(MutationFailure::unknown)?;
    }
    channel.flush().map_err(MutationFailure::unknown)?;
    response::<false>(channel)
}

// Keep XML construction and command-boundary checks in the product layer. The
// pinned generic helpers accept ACKs that explicitly report the wrong raw mode.
fn command(name: &str, attributes: &[(&str, String)]) -> Result<Vec<u8>> {
    let mut element = Element::new(name);
    for (key, value) in attributes {
        element.attributes.insert((*key).into(), value.clone());
    }
    let mut root = Element::new("data");
    root.children.push(XMLNode::Element(element));
    let mut bytes = Vec::new();
    root.write(&mut bytes)?;
    Ok(bytes)
}

fn mutation(
    channel: &mut impl QdlChan,
    name: &str,
    attrs: &[(&str, String)],
) -> Result<(), MutationFailure> {
    let mut bytes = command(name, attrs).map_err(MutationFailure::not_sent)?;
    qdl::firehose_write(channel, &mut bytes).map_err(MutationFailure::unknown)?;
    response::<false>(channel)
}

pub fn patch(
    channel: &mut impl QdlChan,
    offset: u64,
    slot: u8,
    lun: u8,
    size: u64,
    start: &str,
    value: &str,
) -> Result<(), MutationFailure> {
    mutation(
        channel,
        "patch",
        &[
            (
                "SECTOR_SIZE_IN_BYTES",
                channel.fh_config().storage_sector_size.to_string(),
            ),
            ("byte_offset", offset.to_string()),
            ("filename", "DISK".into()),
            ("slot", slot.to_string()),
            ("physical_partition_number", lun.to_string()),
            ("size_in_bytes", size.to_string()),
            ("start_sector", start.into()),
            ("value", value.into()),
        ],
    )
}

pub fn set_bootable(channel: &mut impl QdlChan, lun: u8) -> Result<(), MutationFailure> {
    mutation(
        channel,
        "setbootablestoragedrive",
        &[("value", lun.to_string())],
    )
}

pub fn reset(channel: &mut impl QdlChan) -> Result<(), MutationFailure> {
    mutation(
        channel,
        "power",
        &[("value", "reset".into()), ("DelayInSeconds", "0".into())],
    )
}

pub fn read_storage(
    channel: &mut impl QdlChan,
    out: &mut impl Write,
    sectors: usize,
    slot: u8,
    lun: u8,
    start: u64,
) -> Result<()> {
    read_storage_with_mode(channel, out, sectors, slot, lun, start, WireMode::Direct)
}

pub fn read_storage_with_mode(
    channel: &mut impl QdlChan,
    out: &mut impl Write,
    sectors: usize,
    slot: u8,
    lun: u8,
    start: u64,
    mode: WireMode,
) -> Result<()> {
    let cfg = *channel.fh_config();
    cfg.validate()?;
    let mut left = sectors
        .checked_mul(cfg.storage_sector_size)
        .context("read size overflow")?;
    let mut attributes = vec![
        ("SECTOR_SIZE_IN_BYTES", cfg.storage_sector_size.to_string()),
        ("num_partition_sectors", sectors.to_string()),
        ("slot", slot.to_string()),
        ("physical_partition_number", lun.to_string()),
        ("start_sector", start.to_string()),
    ];
    if mode == WireMode::PreviewBackupGpt {
        attributes.extend([
            ("label", "BackupGPT".into()),
            ("filename", format!("gpt_backup{lun}.bin")),
            ("special_rw_mode", "oplus_gptbackup".into()),
        ]);
    }
    let mut bytes = command("read", &attributes)?;
    qdl::firehose_write(channel, &mut bytes)?;
    let status = qdl::firehose_read(channel, |ch, attrs| {
        let status = qdl::parsers::firehose_parser_ack_nak(ch, attrs)?;
        let expected = if status == FirehoseStatus::Ack {
            "true"
        } else {
            "false"
        };
        if attrs.get("rawmode").is_some_and(|mode| mode != expected) {
            bail!("read response has unexpected rawmode");
        }
        if status == FirehoseStatus::Nak
            && (attrs.contains_key("AttemptRetry") || attrs.contains_key("AttemptRestart"))
        {
            bail!("read rejection has no usable command boundary");
        }
        Ok(status)
    })?;
    if status == FirehoseStatus::Nak {
        return Err(ReadRejected.into());
    }
    let mut buffer = vec![0; left.min(cfg.recv_buffer_size)];
    while left != 0 {
        let size = left.min(buffer.len());
        let received = match channel.read(&mut buffer[..size]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.context("storage read data")?,
        };
        if received == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF in storage data",
            )
            .into());
        }
        out.write_all(&buffer[..received])?;
        left -= received;
    }
    out.flush()?;
    response::<false>(channel)?;
    Ok(())
}
