use crate::{
    output::{Item, Report, Status},
    storage::{self, Partition},
};
use anyhow::{Context, Result, bail};
use qdl::types::QdlChan;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::Path,
};

struct CountWriter<'a> {
    file: &'a mut File,
    accepted: u64,
}
impl Write for CountWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = self.file.write(data)?;
        self.accepted += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
/// Publish only after full data, output flush, final ACK and local sync.
/// Exclusive rename prevents a concurrent file creation from being overwritten.
pub fn save_read(
    path: &Path,
    overwrite: bool,
    item: &mut Item,
    read: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    item.source = Some(path.to_string_lossy().into_owned());
    if path.exists() && !overwrite {
        bail!("output already exists: {}", path.display());
    }
    let parent = path.parent().context("output has no parent")?;
    fs::create_dir_all(parent)?;
    let mut partial = path.as_os_str().to_os_string();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    // Never truncate a prior partial file, even with --overwrite.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let mut output = CountWriter {
        file: &mut file,
        accepted: 0,
    };
    let result = read(&mut output);
    item.transferred_bytes = Some(output.accepted.to_string());
    result?;
    file.sync_all()?;
    drop(file);
    publish(&partial, path, overwrite)?;
    item.execution = "done".into();
    Ok(())
}

fn publish(partial: &Path, path: &Path, overwrite: bool) -> Result<()> {
    if overwrite {
        fs::rename(partial, path).context("cannot replace backup; complete .partial retained")?;
    } else {
        crate::publish::no_replace(partial, path)
            .context("cannot publish backup; complete .partial retained")?;
    }
    Ok(())
}
// Retain every accepted byte in the owned partial file while also providing
// the bounded metadata buffer needed by GPT parsing.
struct CaptureWriter<'a> {
    file: &'a mut File,
    bytes: &'a mut Vec<u8>,
}
impl Write for CaptureWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = self.file.write(data)?;
        self.bytes.extend_from_slice(&data[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
fn capture_primary(
    channel: &mut impl QdlChan,
    lun: u8,
    path: &Path,
    overwrite: bool,
    report: &mut Report,
) -> Result<storage::Table> {
    let sector = channel.fh_config().storage_sector_size as u64;
    let mut partial = path.as_os_str().to_os_string();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    let open = || -> Result<File> {
        fs::create_dir_all(path.parent().context("missing parent")?)?;
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?)
    };
    let mut file = match open() {
        Ok(file) => file,
        Err(e) => {
            report.error(Status::InputError, format!("create GPT partial: {e:#}"));
            return Err(e);
        }
    };
    let before = report.items.len();
    let result = storage::read_gpt_with(channel, |channel, bytes, sectors, start| {
        let mut item = Item::read(lun, None, start * sector, sectors as u64 * sector);
        item.partition_bytes = None;
        item.source = Some(path.to_string_lossy().into_owned());
        let result = (|| -> Result<()> {
            // This is our own file. Replace the completed header probe with
            // the full LBA-0 capture; never reopen/truncate a pre-existing file.
            file.set_len(0)?;
            file.rewind()?;
            let mut out = CaptureWriter {
                file: &mut file,
                bytes,
            };
            qdl::firehose_read_storage(channel, &mut out, sectors, 0, lun, start)
        })();
        item.transferred_bytes = Some(bytes.len().to_string());
        match &result {
            Ok(()) => {
                item.execution = "done".into();
                if start == 1 {
                    item.source = None;
                    item.reason = Some("GPT header probe; superseded by primary capture".into());
                }
            }
            Err(e) => {
                item.reason = Some("partial capture retained".into());
                item.fail(Status::ExecutionFailed, format!("{e:#}"));
            }
        }
        report.items.push(item);
        result
    });
    let result = result.and_then(|table| {
        file.sync_all()?;
        drop(file);
        publish(&partial, path, overwrite)?;
        Ok(table)
    });
    if let Err(e) = &result {
        if report.items.len() > before {
            let item = report.items.last_mut().expect("item exists");
            item.source = Some(path.to_string_lossy().into_owned());
            item.reason = Some("partial capture retained".into());
            if item.errors.is_empty() {
                item.fail(Status::ExecutionFailed, format!("{e:#}"));
            }
        } else {
            report.error(Status::ExecutionFailed, format!("GPT read: {e:#}"));
        }
    }
    report.summarize();
    result
}
pub fn single_partition(
    channel: &mut impl QdlChan,
    lun: u8,
    name: &str,
    output: &Path,
    overwrite: bool,
    report: &mut Report,
) {
    directory(
        channel,
        &[lun],
        &[name.to_owned()],
        output,
        overwrite,
        report,
    );
}

/// Reads only the supplied discovered scope. A failed transport stops the session.
pub fn directory(
    channel: &mut impl QdlChan,
    luns: &[u8],
    names: &[String],
    output: &Path,
    overwrite: bool,
    report: &mut Report,
) {
    report.coverage.limited = true;
    report.coverage.reason = Some(format!(
        "requested LUNs {luns:?}; per-read results define scanned scope; only listed GPT partitions"
    ));
    let mut found = std::collections::HashSet::new();
    let mut complete = true;
    for &lun in luns {
        if !backup_lun(channel, lun, names, output, overwrite, report, &mut found) {
            complete = false;
            break;
        }
    }
    for name in names {
        if complete && !found.contains(name) {
            report.error(
                Status::InputError,
                format!("selected partition not found in scanned scope: {name}"),
            );
        }
    }
    report.summarize();
}

fn backup_lun(
    channel: &mut impl QdlChan,
    lun: u8,
    names: &[String],
    output: &Path,
    overwrite: bool,
    report: &mut Report,
    found: &mut std::collections::HashSet<String>,
) -> bool {
    let dir = output.join(format!("lun{lun}"));
    let primary_path = dir.join(".gpt/primary.bin");
    let backup_path = dir.join(".gpt/backup.bin");
    for path in [&primary_path, &backup_path] {
        let mut partial = path.as_os_str().to_os_string();
        partial.push(".partial");
        if (!overwrite && path.exists()) || Path::new(&partial).exists() {
            report.error(
                Status::InputError,
                format!("output exists: {} (or .partial)", path.display()),
            );
            return false;
        }
    }
    let table = match capture_primary(channel, lun, &primary_path, overwrite, report) {
        Ok(table) => table,
        Err(_) => {
            report.summarize();
            return false;
        }
    };
    let selected = storage::partitions(&table, lun).expect("validated GPT");
    let selected: Vec<_> = selected
        .into_iter()
        .filter(|p| names.is_empty() || names.contains(&p.name))
        .collect();
    for p in &selected {
        found.insert(p.name.clone());
    }
    if selected.is_empty() {
        return true;
    }
    if selected
        .iter()
        .all(|p| !storage::safe_name(&p.name) || storage::name_conflicts(&selected, p))
    {
        for p in &selected {
            let mut item = Item::read(
                lun,
                Some(p.name.clone()),
                p.start_byte().expect("validated GPT"),
                p.bytes().expect("validated GPT"),
            );
            item.fail(
                Status::Unsupported,
                "partition name is unsafe or conflicts within selected LUN",
            );
            report.items.push(item);
        }
        return true;
    }
    let h = &table.gpt.header;
    let backup_start = h.last_usable_lba + 1;
    let backup_sectors = h.backup_lba - h.last_usable_lba;
    let sector = table.gpt.sector_size;
    let mut backup = Item::read(lun, None, backup_start * sector, backup_sectors * sector);
    backup.partition_bytes = None;
    backup.source = Some(backup_path.to_string_lossy().into_owned());
    // The metadata span is bounded independently of the disk size.
    if backup_sectors * sector > 16 * 1024 * 1024 {
        backup.fail(Status::Unsupported, "backup GPT capture exceeds 16 MiB");
    } else if let Err(e) = save_read(&backup_path, overwrite, &mut backup, |mut out| {
        qdl::firehose_read_storage(
            channel,
            &mut out,
            usize::try_from(backup_sectors)?,
            0,
            lun,
            backup_start,
        )
    }) {
        backup.fail(Status::ExecutionFailed, e.to_string());
    }
    let mut stopped = !backup.errors.is_empty();
    report.items.push(backup);
    for p in &selected {
        let partition_path = dir.join(format!("{}.bin", p.name));
        let mut item = Item::read(
            lun,
            Some(p.name.clone()),
            p.start_byte().expect("validated GPT"),
            p.bytes().expect("validated GPT"),
        );
        if !storage::safe_name(&p.name) || storage::name_conflicts(&selected, p) {
            item.fail(
                Status::Unsupported,
                "partition name is unsafe or conflicts within selected LUN",
            );
        } else {
            item.source = Some(partition_path.to_string_lossy().into_owned());
            if stopped {
                item.reason = Some("previous read failed; session stopped".into());
            } else if output_exists(&partition_path, overwrite) {
                item.fail(Status::InputError, "output or partial already exists");
            } else if let Err(e) = save_partition(channel, p, &partition_path, overwrite, &mut item)
            {
                item.fail(Status::ExecutionFailed, format!("{e:#}"));
                stopped = true;
            }
        }
        report.items.push(item);
    }
    report.summarize();
    !stopped
}
fn output_exists(path: &Path, overwrite: bool) -> bool {
    let mut partial = path.as_os_str().to_os_string();
    partial.push(".partial");
    (!overwrite && path.exists()) || Path::new(&partial).exists()
}
pub fn save_partition(
    channel: &mut impl QdlChan,
    p: &Partition,
    path: &Path,
    overwrite: bool,
    item: &mut Item,
) -> Result<()> {
    save_read(path, overwrite, item, |mut out| {
        storage::read_partition(channel, p, &mut out)
    })
}
