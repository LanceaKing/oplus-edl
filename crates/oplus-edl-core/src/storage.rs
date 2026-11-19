// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2026 LanceaKing.
//! Product storage policies and transport adapters. Generic operations live in qdl.
use crate::result::ExecutionState;
use crate::{
    firehose::WireMode,
    model::{PartitionKey, SessionId},
    result::{Item, Report, Status},
    service::Job,
    session::{Stopped, check_start},
};
use anyhow::{Context, Result, bail};
pub use qdl::storage::{Partition, Table, find_partition, partitions, read_gpt_with};
use qdl::types::{MutationFailure, QdlChan, ReadRejected};
use std::io::{Read, Write};

pub fn read_gpt(channel: &mut impl QdlChan, lun: u8) -> Result<Table> {
    read_gpt_with(channel, |ch, out, sectors, start| {
        crate::firehose::read_storage(ch, out, sectors, 0, lun, start)
    })
}

pub(crate) fn snapshot_partitions(
    table: &Table,
    lun: u8,
    generation: SessionId,
    revision: u64,
) -> Vec<PartitionKey> {
    table
        .gpt
        .iter()
        .filter(|(_, partition)| partition.is_used())
        .map(|(entry, partition)| PartitionKey {
            generation,
            snapshot_revision: revision,
            lun,
            entry,
            name: partition.partition_name.as_str().to_owned(),
            start_sector: partition.starting_lba.to_string(),
            sectors: (partition.ending_lba - partition.starting_lba + 1).to_string(),
            sector_size: table.gpt.sector_size as usize,
        })
        .collect()
}

pub(crate) fn read_gpt_recorded(
    channel: &mut impl QdlChan,
    lun: u8,
    mode: WireMode,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<Table> {
    let before = report.items.len();
    read_gpt_with(channel, |channel, out, sectors, start| {
        read_recorded(channel, out, lun, start, sectors, mode, job, report)
    })
    .inspect_err(|error| {
        if !error.is::<Stopped>()
            && report.items[before..]
                .iter()
                .all(|item| item.errors.is_empty())
        {
            report.error_source(Status::ExecutionFailed, error);
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_recorded(
    channel: &mut impl QdlChan,
    out: &mut Vec<u8>,
    lun: u8,
    start: u64,
    sectors: usize,
    mode: WireMode,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<()> {
    check_start(job)?;
    let sector = channel.fh_config().storage_sector_size as u64;
    let mut item = Item::read(lun, None, start * sector, sectors as u64 * sector);
    let before = out.len();
    let result =
        crate::firehose::read_storage_with_mode(channel, out, sectors, 0, lun, start, mode);
    item.transferred_bytes = Some((out.len() - before).to_string());
    record_read(report, item, &result);
    result
}

pub(crate) fn record_read<T>(report: &mut Report, mut item: Item, result: &Result<T>) {
    match result {
        Ok(_) => item.execution = ExecutionState::Done,
        Err(error) => {
            report.unusable |= !error.is::<ReadRejected>();
            item.fail_source(Status::ExecutionFailed, error);
        }
    }
    report.items.push(item);
}

pub(crate) fn query_info(
    channel: &mut impl QdlChan,
    lun: u8,
    job: &Job,
    report: &mut Report,
) -> Result<qdl::FirehoseStorageInfo> {
    check_start(Some(job))?;
    let result = qdl::firehose_get_storage_info(channel, lun)
        .and_then(|info| {
            crate::firehose::check_raw_mode(
                info.attributes.get("rawmode").map(String::as_str),
                false,
            )?;
            Ok(info)
        })
        .context("getstorageinfo");
    match &result {
        Ok(_) => {
            let mut item = Item::read(lun, None, 0, 0);
            item.operation = "getstorageinfo".into();
            item.execution = ExecutionState::Done;
            item.start_byte = None;
            item.partition_bytes = None;
            item.transfer_bytes = None;
            item.transferred_bytes = None;
            report.items.push(item);
        }
        Err(error) => {
            report.unusable = true;
            report.error_source(Status::ExecutionFailed, error);
        }
    }
    result
}
pub fn read_partition(
    channel: &mut impl QdlChan,
    p: &Partition,
    out: &mut impl Write,
) -> Result<()> {
    qdl::storage::read_partition_with(channel, p, out, |ch, out, sectors, lun, start| {
        crate::firehose::read_storage(ch, out, sectors, 0, lun, start)
    })
}
pub fn program_partition(
    channel: &mut impl QdlChan,
    p: &Partition,
    source: &mut impl Read,
    source_bytes: u64,
) -> Result<(), MutationFailure> {
    qdl::storage::program_partition(channel, p, source, source_bytes, 0)
}

pub fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
/// Directory names share one collision rule in both directions.
pub fn name_conflicts<'a>(
    partitions: impl IntoIterator<Item = &'a Partition>,
    p: &Partition,
) -> bool {
    partitions
        .into_iter()
        .filter(|other| other.lun == p.lun && other.name.eq_ignore_ascii_case(&p.name))
        .count()
        > 1
}
pub fn lun_directory(name: &str) -> Option<u8> {
    let value = name.strip_prefix("lun")?.parse::<u8>().ok()?;
    (name == format!("lun{value}")).then_some(value)
}
/// Recognize explicit storage counts, never infer LUNs by probing until failure.
pub fn discovered_luns(info: &qdl::FirehoseStorageInfo, sector_size: usize) -> Result<Vec<u8>> {
    let mut counts = Vec::new();
    for key in ["num_physical_partitions", "num_physical", "bNumberLu"] {
        if let Some(v) = info.attributes.get(key) {
            counts.push(v.parse::<u16>()?);
        }
    }
    for log in &info.logs {
        let text = log.trim().strip_prefix("INFO:").unwrap_or(log).trim();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(storage) = json.get("storage_info")
        {
            if let Some(n) = storage.get("num_physical") {
                let n = n
                    .as_u64()
                    .or_else(|| n.as_str()?.parse().ok())
                    .context("invalid LUN count")?;
                counts.push(u16::try_from(n)?);
            }
            if let Some(size) = storage.get("block_size") {
                let size = size
                    .as_u64()
                    .or_else(|| size.as_str()?.parse().ok())
                    .context("invalid block size")?;
                if size != sector_size as u64 {
                    bail!("storage sector size conflicts with configuration");
                }
            }
        }
    }
    let count = *counts.first().context(
        "storage information has no supported LUN count; specify --lun to select a limited scope",
    )?;
    if count == 0 || count > 256 || counts.iter().any(|n| *n != count) {
        bail!("invalid or conflicting LUN count");
    }
    Ok((0..count).map(|n| n as u8).collect())
}
