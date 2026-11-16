//! Read-only package/device binding before any package storage mutation.
use crate::{
    firehose::{self, WireMode},
    program_xml::{Candidate, Operation},
    storage,
};
use anyhow::{Context, Result, ensure};
use qdl::types::QdlChan;

struct PlannedRange {
    lun: u8,
    start: String,
    offset: u64,
    bytes: u64,
}

/// Retain the package identity and ranges while Writes owns the opened images.
pub struct PackageGuard {
    project: u32,
    ranges: Vec<PlannedRange>,
}

impl PackageGuard {
    pub fn new(project: u32, candidates: &[Candidate]) -> Result<Self> {
        ensure!(
            (1..=i32::MAX as u32).contains(&project),
            "invalid package project"
        );
        let mut ranges = Vec::new();
        for c in candidates {
            let (offset, bytes) = match &c.operation {
                Operation::Program { span_bytes, .. } => (0, *span_bytes),
                Operation::Patch {
                    byte_offset, size, ..
                } => (*byte_offset, *size),
                Operation::Skip { .. } => continue,
            };
            ensure!(
                c.lun < 5,
                "package mutation targets protected or unsupported LUN"
            );
            ensure!(
                c.sector_size == 4096 && c.slot == 0 && bytes > 0,
                "invalid package mutation geometry"
            );
            ranges.push(PlannedRange {
                lun: c.lun,
                start: c.start_sector.clone(),
                offset,
                bytes,
            });
        }
        Ok(Self { project, ranges })
    }

    /// The current SFP profile has six UFS LUNs. Read every primary GPT so an
    /// absent/duplicate OCDT cannot silently select a different partition.
    /// No program, patch, boot-selection or reset command is issued here.
    pub fn validate(&self, channel: &mut impl QdlChan, mode: WireMode) -> Result<()> {
        ensure!(
            matches!(
                channel.fh_config().storage_type,
                qdl::types::FirehoseStorageType::Ufs
            ) && channel.fh_config().storage_sector_size == 4096,
            "package project validation requires the six-LUN UFS profile"
        );
        let sector = 4096u64;
        let mut ocdt = None;
        for lun in 0..6 {
            let table = storage::read_gpt_with(channel, |ch, out, count, start| {
                firehose::read_storage_with_mode(ch, out, count, 0, lun, start, mode)
            })
            .with_context(|| format!("project validation: read GPT LUN {lun}"))?;
            let disk_sectors = table
                .gpt
                .header
                .backup_lba
                .checked_add(1)
                .context("disk size overflow")?;
            let disk_bytes = disk_sectors
                .checked_mul(sector)
                .context("disk byte size overflow")?;
            let protected: Vec<_> = table
                .gpt
                .iter()
                .filter(|(_, p)| {
                    p.is_used() && p.partition_name.as_str().eq_ignore_ascii_case("ocdt")
                })
                .map(|(_, p)| (p.starting_lba, p.ending_lba))
                .collect();
            for r in self.ranges.iter().filter(|r| r.lun == lun) {
                let start = resolve_sector(&r.start, disk_sectors)?
                    .checked_mul(sector)
                    .and_then(|v| v.checked_add(r.offset))
                    .context("mutation offset overflow")?;
                let end = start
                    .checked_add(r.bytes)
                    .context("mutation end overflow")?;
                ensure!(
                    end <= disk_bytes,
                    "package mutation exceeds device LUN {lun}"
                );
                for &(first, last) in &protected {
                    ensure!(
                        end <= first * sector || start >= (last + 1) * sector,
                        "package mutation overlaps device OCDT on LUN {lun}"
                    );
                }
            }
            for (start, last) in protected {
                ensure!(ocdt.is_none(), "ambiguous device OCDT partitions");
                ocdt = Some((lun, start, last - start + 1));
            }
        }
        let (lun, start, sectors) = ocdt.context("device OCDT partition not found")?;
        let mut header = Vec::new();
        firehose::read_storage_with_mode(channel, &mut header, 1, 0, lun, start, mode)
            .context("read device OCDT header")?;
        let (_, end) = payload_range(&header)?;
        let count = end.div_ceil(sector as usize);
        ensure!(
            count as u64 <= sectors,
            "OCDT payload exceeds its partition"
        );
        let data = if count == 1 {
            header
        } else {
            let mut data = Vec::new();
            firehose::read_storage_with_mode(channel, &mut data, count, 0, lun, start, mode)
                .context("read device OCDT payload")?;
            ensure!(
                data.get(..16) == header.get(..16),
                "OCDT header changed between reads"
            );
            data
        };
        let actual = extract_project(&data)?;
        ensure!(
            actual == self.project,
            "device OCDT project {actual} does not match package project {}",
            self.project
        );
        Ok(())
    }
}

fn resolve_sector(text: &str, disk: u64) -> Result<u64> {
    let text = text.strip_suffix('.').unwrap_or(text);
    if let Some(tail) = text.strip_prefix("NUM_DISK_SECTORS") {
        if tail.is_empty() {
            return Ok(disk);
        }
        if let Some(n) = tail.strip_prefix('-') {
            return disk
                .checked_sub(n.parse()?)
                .context("disk-relative sector underflow");
        }
        if let Some(n) = tail.strip_prefix('+') {
            return disk
                .checked_add(n.parse()?)
                .context("disk-relative sector overflow");
        }
        anyhow::bail!("unsupported disk-relative sector");
    }
    text.parse().context("invalid package sector")
}

fn payload_range(data: &[u8]) -> Result<(usize, usize)> {
    // Binary Ninja: Oplus_Qualcomm.dll!oplus_ocdt_extract_project_id
    // 0x180007680: LE magic, u16 offset at +8, u16 length at +12.
    ensure!(
        data.len() >= 16 && &data[..4] == b"TDCO",
        "invalid OCDT header"
    );
    let offset = u16::from_le_bytes(data[8..10].try_into()?) as usize;
    let length = u16::from_le_bytes(data[12..14].try_into()?) as usize;
    ensure!(
        offset >= 16 && length >= 12 && length.is_multiple_of(4),
        "invalid OCDT payload range"
    );
    Ok((offset, offset + length))
}

/// Read-only decoder. This is not an authenticity check or an OCDT writer.
pub fn extract_project(data: &[u8]) -> Result<u32> {
    let (offset, end) = payload_range(data)?;
    let payload = data.get(offset..end).context("truncated OCDT payload")?;
    // 0x180007010 supplies this repeating format key; extractor rotates then XORs.
    let key = b"geyixue";
    let mut project = None;
    let mut copies = 0;
    for (i, word) in payload.as_chunks::<4>().0.iter().enumerate() {
        let mut decoded = [0u8; 4];
        for j in 0..4 {
            decoded[j] = word[j].rotate_right(4) ^ key[(4 * i + j) % key.len()];
        }
        let value = u32::from_le_bytes(decoded);
        if value == 0 {
            continue;
        }
        ensure!(value <= i32::MAX as u32, "invalid OCDT project number");
        ensure!(
            project.is_none_or(|n| n == value),
            "conflicting OCDT project copies"
        );
        project = Some(value);
        copies += 1;
    }
    ensure!(
        copies >= 3,
        "OCDT needs at least three matching project copies"
    );
    project.context("missing OCDT project")
}
