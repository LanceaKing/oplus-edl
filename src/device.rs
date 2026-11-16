use crate::cli::{Backend, Cli, Memory};
use anyhow::{Context, Result, bail};
use qdl::types::{
    FirehoseConfiguration, FirehoseStatus, FirehoseStorageType, QdlBackend, QdlChan, QdlDevice,
    QdlReadWrite,
};
use std::{io::Read, path::Path};

const MAX_SIGNED_DIGEST_BYTES: usize = 16 * 1024 * 1024;

// VIP loaders request the table instead of emitting the standard welcome end marker.
// Return at that request, without adding an idle USB timeout to the handshake.
fn digest_welcome(channel: &mut impl QdlChan) -> Result<Vec<String>> {
    let limit = channel.fh_config().xml_buf_size;
    anyhow::ensure!(
        (1..=MAX_SIGNED_DIGEST_BYTES).contains(&limit),
        "invalid welcome XML limit"
    );
    let mut logs = Vec::new();
    let mut total = 0usize;
    loop {
        let mut packet = Vec::new();
        loop {
            let mut byte = [0];
            match channel.read_exact(&mut byte) {
                Err(e) if packet.is_empty() && e.kind() == std::io::ErrorKind::TimedOut => {
                    return Ok(logs);
                }
                other => other?,
            }
            if packet.is_empty() && byte[0].is_ascii_whitespace() {
                continue;
            }
            packet.push(byte[0]);
            anyhow::ensure!(packet.len() <= limit, "welcome XML exceeds receive limit");
            if packet.ends_with(b"</data>") {
                break;
            }
        }
        total += packet.len();
        anyhow::ensure!(
            total <= MAX_SIGNED_DIGEST_BYTES,
            "welcome logs exceed limit"
        );
        let xml = xmltree::Element::parse(packet.as_slice()).context("invalid welcome XML")?;
        let nodes: Vec<_> = xml
            .children
            .iter()
            .filter_map(xmltree::XMLNode::as_element)
            .collect();
        anyhow::ensure!(
            xml.name == "data" && nodes.len() == 1 && nodes[0].name == "log",
            "expected a welcome log"
        );
        let message = nodes[0]
            .attributes
            .get("value")
            .cloned()
            .unwrap_or_default();
        let done = message.starts_with("INFO: End of supported functions")
            || message.starts_with("INFO: VIP is enabled, receiving the partition info of size ");
        logs.push(message);
        if done {
            return Ok(logs);
        }
    }
}

pub fn load_signed_digest(path: &Path) -> Result<Vec<u8>> {
    let file = crate::input::open_regular(path)?;
    let mut bytes = Vec::new();
    file.take((MAX_SIGNED_DIGEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read signed digest {}", path.display()))?;
    validate_signed_digest(&bytes)?;
    Ok(bytes)
}

fn validate_signed_digest(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_SIGNED_DIGEST_BYTES {
        bail!("signed digest must contain 1 to {MAX_SIGNED_DIGEST_BYTES} bytes");
    }
    Ok(())
}

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
    initialize_with_progress(channel, images, None, |_| {})
}

/// Reports connection progress without enabling the dependency's stdout output.
pub fn initialize_with_progress(
    channel: &mut impl QdlChan,
    images: &mut [Option<Vec<u8>>],
    signed_digest: Option<&[u8]>,
    progress: impl FnMut(&str),
) -> Result<()> {
    initialize_with_digest_signature(channel, images, signed_digest, None, progress)
}

/// The preview DIGEST protocol consumes a matching loader/table/signature set.
/// ACKs confirm protocol steps, not storage permissions or firmware compatibility.
pub fn initialize_with_digest_signature(
    channel: &mut impl QdlChan,
    images: &mut [Option<Vec<u8>>],
    signed_digest: Option<&[u8]>,
    signature: Option<&[u8]>,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    channel.mut_fh_config().quiet = true;
    channel.fh_config().validate()?;
    if let Some(bytes) = signed_digest {
        validate_signed_digest(bytes)?;
    }
    if let Some(bytes) = signature {
        if signed_digest.is_none() {
            bail!("DIGEST signature requires a matching signed digest table");
        }
        validate_signed_digest(bytes).context("DIGEST signature")?;
    }
    // Requires the reviewed loader-only API: removing the local patch before
    // updating the pinned revision must fail to build, not restore unsafe dispatch.
    progress("Sahara: uploading programmer");
    qdl::sahara::sahara_load_programmer(channel, images).context("Sahara programmer upload")?;
    progress("Firehose: reading startup logs");
    let logs = if signature.is_some() {
        digest_welcome(channel)
    } else {
        qdl::firehose_read_welcome(channel)
    }
    .context("Firehose startup")?;
    for log in &logs {
        progress(log);
    }
    if let Some(bytes) = signed_digest {
        progress("Firehose: sending signed digest table");
        // This is binary data. The XML writer may append a newline to packets
        // divisible by 512; changing even one byte would invalidate this input.
        channel
            .write_all(bytes)
            .context("signed digest transmission")?;
        channel.flush().context("signed digest transmission")?;
        if let Some(signature) = signature {
            // Downloaded SM8650 loader: 0x149a4320 validates the table then
            // 0x149a42c4 sends an ACK through blocking TX (0x149a461c).
            // Drain that response before sending verify; otherwise USB OUT can
            // stall while the loader waits for its IN response to complete.
            progress("Firehose: waiting for signed digest acknowledgement");
            digest_ack::<false>(channel).context("DIGEST table response")?;
            progress("Firehose: sending DIGEST verify and matching signature");
            qdl::firehose_write(channel, &mut b"<?xml version=\"1.0\"?><data><verify value=\"ping\" EnableVip=\"1\"/></data>".to_vec())
                .context("DIGEST verify command")?;
            // verify handler 0x149a185c calls 0x149a1ac4: ACK rawmode=true
            // precedes the 4096-byte signature receive in 0x149a1218.
            digest_ack::<true>(channel).context("DIGEST verify ready response")?;
            channel
                .write_all(signature)
                .context("DIGEST signature transmission")?;
            if channel.fh_config().backend == QdlBackend::Usb && signature.len().is_multiple_of(512)
            {
                // A full-packet binary transfer needs a USB short-packet terminator.
                // Do not append a byte to the signed material.
                channel
                    .write(&[])
                    .context("DIGEST signature USB terminator")?;
            }
            channel.flush().context("DIGEST signature transmission")?;
            progress("Firehose: waiting for DIGEST signature acknowledgement");
            digest_ack::<false>(channel).context("DIGEST signature response")?;
            progress("Firehose: initializing DIGEST hashes");
            qdl::firehose_write(
                channel,
                &mut b"<?xml version=\"1.0\"?><data><sha256init value=\"ping\"/></data>".to_vec(),
            )
            .context("DIGEST sha256init command")?;
            digest_ack::<false>(channel).context("DIGEST sha256init response")?;
            progress("Firehose: DIGEST handshake acknowledged; storage access not yet verified");
        } else {
            let status = qdl::firehose_read(channel, |channel, attrs| {
                let status = qdl::parsers::firehose_parser_ack_nak(channel, attrs)?;
                if attrs.get("rawmode").is_some_and(|mode| mode != "false") {
                    bail!("signed digest response did not return to XML command mode");
                }
                Ok(status)
            })
            .context("signed digest response")?;
            if status != FirehoseStatus::Ack {
                bail!("signed digest table rejected");
            }
            progress(
                "Firehose: signed digest table acknowledged; session authorization not verified",
            );
        }
    }
    progress("Firehose: configuring storage");
    configure(channel).context("Firehose configuration")?;
    progress("Firehose: configured; storage access not yet verified");
    Ok(())
}

fn digest_ack<const RAW: bool>(channel: &mut impl QdlChan) -> Result<()> {
    let status = qdl::firehose_read(channel, |channel, attrs| {
        let status = qdl::parsers::firehose_parser_ack_nak(channel, attrs)?;
        if attrs
            .get("rawmode")
            .is_some_and(|mode| mode != if RAW { "true" } else { "false" })
            || (RAW && !attrs.contains_key("rawmode"))
        {
            bail!("DIGEST response has unexpected raw mode");
        }
        Ok(status)
    })?;
    if status != FirehoseStatus::Ack {
        bail!("DIGEST command rejected");
    }
    Ok(())
}

/// Check the UFS boot capability before a preview flash modifies storage.
/// A physical LUN index is not the same as its UFS Boot LUN ID.
pub fn validate_preview_boot_lun(channel: &mut impl QdlChan, lun: u8) -> Result<()> {
    if !matches!(channel.fh_config().storage_type, FirehoseStorageType::Ufs) {
        return Ok(());
    }
    let info = qdl::firehose_get_storage_info(channel, lun)?;
    let ids: Vec<_> = info
        .logs
        .iter()
        .filter_map(|s| s.strip_prefix("INFO: UFS Boot LUN ID: = "))
        .collect();
    anyhow::ensure!(
        ids.len() == 1,
        "preview loader did not report one UFS Boot LUN ID"
    );
    let text = ids[0].trim();
    let id = u8::from_str_radix(
        text.strip_prefix("0x").context("invalid UFS Boot LUN ID")?,
        16,
    )?;
    anyhow::ensure!(
        matches!(id, 1 | 2),
        "LUN {lun} is not a UFS boot LUN (Boot LUN ID {id}); select the LUN containing the boot firmware"
    );
    Ok(())
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

/// Wait only before opening a session. Never reconnect or replay a failed write.
/// USB enumeration is EDL-filtered; serial enumeration includes unrelated ports,
/// so serial callers must specify the exact port.
pub fn wait_for_edl(
    backend: Backend,
    selected: Option<&str>,
    timeout: Option<std::time::Duration>,
    mut observed: impl FnMut(&[String]),
) -> Result<String> {
    if matches!(backend, Backend::Serial) && selected.is_none() {
        bail!("serial EDL waiting requires an explicit --device port");
    }
    wait_for_candidate(
        selected,
        timeout,
        || {
            let keys = candidates(backend).context("enumerate EDL devices")?;
            observed(&keys);
            Ok(keys)
        },
        std::thread::sleep,
    )
}

fn wait_for_candidate(
    selected: Option<&str>,
    timeout: Option<std::time::Duration>,
    mut scan: impl FnMut() -> Result<Vec<String>>,
    mut pause: impl FnMut(std::time::Duration),
) -> Result<String> {
    use std::time::{Duration, Instant};
    if selected.is_some_and(str::is_empty) {
        bail!("device selector must not be empty");
    }
    let started = Instant::now();
    loop {
        let keys = scan()?;
        let matching: Vec<_> = keys
            .iter()
            .filter(|key| selected.is_none_or(|s| s == key.as_str()))
            .collect();
        match matching.as_slice() {
            [key] => return Ok((*key).clone()),
            [] => {}
            _ => bail!("multiple matching EDL devices; specify a unique --device"),
        }
        let interval = Duration::from_millis(250);
        let delay = if let Some(limit) = timeout {
            let remaining = limit.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                bail!(
                    "timed out waiting for EDL device{}",
                    selected.map(|s| format!(" {s}")).unwrap_or_default()
                );
            }
            interval.min(remaining)
        } else {
            interval
        };
        pause(delay);
    }
}

#[cfg(test)]
mod wait_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn disconnected_device_is_polled_until_connected() {
        let mut scans = 0;
        let mut pauses = 0;
        let selected = wait_for_candidate(
            None,
            None,
            || {
                scans += 1;
                Ok(if scans < 3 {
                    vec![]
                } else {
                    vec!["usb-edl".into()]
                })
            },
            |d| {
                assert_eq!(d, Duration::from_millis(250));
                pauses += 1;
            },
        )
        .unwrap();
        assert_eq!(selected, "usb-edl");
        assert_eq!((scans, pauses), (3, 2));
    }
    #[test]
    fn explicit_selector_waits_for_that_device_not_another() {
        let mut scans = 0;
        let selected = wait_for_candidate(
            Some("wanted"),
            None,
            || {
                scans += 1;
                Ok(if scans == 1 {
                    vec!["other".into()]
                } else {
                    vec!["other".into(), "wanted".into()]
                })
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(selected, "wanted");
        assert_eq!(scans, 2);
    }
    #[test]
    fn ambiguity_errors_and_timeout_do_not_retry_or_open() {
        for (selector, keys) in [
            (None, vec!["a".into(), "b".into()]),
            (Some("a"), vec!["a".into(), "a".into()]),
        ] {
            assert!(
                wait_for_candidate(
                    selector,
                    None,
                    || Ok(keys.clone()),
                    |_| panic!("must not wait on ambiguity")
                )
                .unwrap_err()
                .to_string()
                .contains("multiple")
            );
        }
        assert!(
            wait_for_candidate(
                None,
                Some(Duration::ZERO),
                || Ok(vec![]),
                |_| panic!("deadline reached")
            )
            .unwrap_err()
            .to_string()
            .contains("timed out")
        );
        assert!(
            wait_for_candidate(
                None,
                None,
                || bail!("enumeration failure"),
                |_| panic!("must not hide errors")
            )
            .unwrap_err()
            .to_string()
            .contains("enumeration failure")
        );
        assert!(
            wait_for_edl(Backend::Serial, None, None, |_| panic!(
                "must not enumerate arbitrary ports"
            ))
            .is_err()
        );
    }
}
