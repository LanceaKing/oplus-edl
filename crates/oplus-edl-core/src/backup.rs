use crate::result::ExecutionState;
use crate::{
    input::{PathUse, resolve_directory},
    model::{PartitionSelection, PartitionTargets},
    result::{Item, OutputState, Report, Status},
    service::Job,
    session::{Stopped, check_dispatch, check_start},
    storage::{self, Partition},
};
use anyhow::{Context, Result, ensure};
use qdl::types::{QdlChan, ReadRejected};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
};

struct CountWriter<'a> {
    file: &'a mut File,
    accepted: u64,
    job: Option<(&'a Job, u64)>,
}
impl Write for CountWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = self.file.write(data)?;
        self.accepted += n as u64;
        if let Some((job, expected)) = self.job {
            job.progress("read", Some((self.accepted, expected)));
        }
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
    save_capture(
        path,
        overwrite,
        item,
        &mut CaptureProgress::default(),
        None,
        read,
    )
}

#[derive(Default)]
struct CaptureProgress {
    partial: Option<PathBuf>,
    read_started: bool,
    ack: bool,
}

impl CaptureProgress {
    fn record(self, report: &mut Report, path: &Path, kind: &'static str, result: &Result<()>) {
        if let Err(error) = result {
            if let Some(partial) = self.partial {
                report.output(partial, kind, OutputState::Partial);
            }
            report.unusable |= self.read_started && !self.ack && !error.is::<ReadRejected>();
        } else {
            report.output(path.to_owned(), kind, OutputState::Complete);
        }
    }
}

#[derive(Debug)]
struct OutputCollision(PathBuf);

impl std::fmt::Display for OutputCollision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "output or partial already exists: {}", self.0.display())
    }
}

impl std::error::Error for OutputCollision {}

fn partial_path(path: &Path) -> PathBuf {
    let mut partial = path.as_os_str().to_os_string();
    partial.push(".partial");
    partial.into()
}

fn require_vacant(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(OutputCollision(path.to_owned()).into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("check backup output {}", path.display())),
    }
}

fn check_final_path(path: &Path, overwrite: bool, job: Option<&Job>) -> Result<()> {
    if !overwrite {
        require_vacant(path)?;
    }
    check_start(job)
}

fn save_capture(
    path: &Path,
    overwrite: bool,
    item: &mut Item,
    progress: &mut CaptureProgress,
    job: Option<&Job>,
    read: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    item.source = Some(path.to_owned());
    let parent = path.parent().context("output has no parent")?;
    check_bound_directory(parent, job)?;
    if !overwrite {
        require_vacant(path)?;
    }
    fs::create_dir_all(parent)?;
    check_bound_directory(parent, job)?;
    let partial = partial_path(path);
    require_vacant(&partial)?;
    // Never truncate a prior partial file, even with --overwrite.
    if item.partition.is_some() {
        check_dispatch(job, "partition output open")?;
    }
    check_bound_directory(parent, job)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| -> anyhow::Error {
            if error.kind() == io::ErrorKind::AlreadyExists {
                OutputCollision(partial.clone()).into()
            } else {
                error.into()
            }
        })?;
    progress.partial = Some(partial.clone());
    let mut output = CountWriter {
        file: &mut file,
        accepted: 0,
        job: job.zip(
            item.transfer_bytes
                .as_deref()
                .and_then(|size| size.parse().ok()),
        ),
    };
    if let Some((job, expected)) = output.job {
        job.progress("read", Some((0, expected)));
    }
    check_dispatch(
        job,
        if item.partition.is_some() {
            "partition read"
        } else {
            "backup GPT read"
        },
    )?;
    check_bound_directory(parent, job)?;
    check_final_path(path, overwrite, job)?;
    progress.read_started = true;
    let result = read(&mut output);
    item.transferred_bytes = Some(output.accepted.to_string());
    result?;
    progress.ack = true;
    file.sync_all()?;
    drop(file);
    check_bound_directory(parent, job)?;
    publish(&partial, path, overwrite)?;
    item.execution = ExecutionState::Done;
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
    replace_probe: bool,
}
impl Write for CaptureWriter<'_> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        // Keep the completed probe until the second exchange supplies actual data.
        if self.replace_probe && !data.is_empty() {
            self.file.rewind()?;
            self.file.set_len(0)?;
            self.replace_probe = false;
        }
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
    job: Option<&Job>,
    report: &mut Report,
) -> Result<storage::Table> {
    check_start(job)?;
    let sector = channel.fh_config().storage_sector_size as u64;
    let partial = partial_path(path);
    let parent = path.parent().context("missing parent")?;
    let open = || -> Result<File> {
        check_bound_directory(parent, job)?;
        if !overwrite {
            require_vacant(path)?;
        }
        fs::create_dir_all(parent)?;
        check_bound_directory(parent, job)?;
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?)
    };
    let mut file = None;
    let before = report.items.len();
    let errors_before = report.errors.len();
    let result = storage::read_gpt_with(channel, |channel, bytes, sectors, start| {
        check_start(job)?;
        let replace_probe = file.is_some();
        if !replace_probe {
            let opened = open().context("create GPT partial").inspect_err(|error| {
                report.error_source(Status::InputError, error);
            })?;
            file = Some(opened);
            report.output(partial.clone(), "gpt", OutputState::Partial);
        }
        let mut item = Item::read(lun, None, start * sector, sectors as u64 * sector);
        item.partition_bytes = None;
        item.source = Some(path.to_owned());
        let mut out = CaptureWriter {
            file: file.as_mut().expect("capture file open"),
            bytes,
            replace_probe,
        };
        check_dispatch(
            job,
            if replace_probe {
                "primary GPT read"
            } else {
                "GPT probe"
            },
        )?;
        check_bound_directory(parent, job)?;
        check_final_path(path, overwrite, job).inspect_err(|error| {
            if error.is::<OutputCollision>() {
                report.error_source(Status::InputError, error);
            }
        })?;
        let result = crate::firehose::read_storage(channel, &mut out, sectors, 0, lun, start);
        // A dispatched full capture owns the partial even if its read fails before data.
        if out.replace_probe
            && result.is_err()
            && let Err(error) = out.file.set_len(0)
        {
            report.error_source(Status::ExecutionFailed, &error.into());
        }
        item.transferred_bytes = Some(bytes.len().to_string());
        match &result {
            Ok(()) => {
                item.execution = ExecutionState::Done;
                if start == 1 {
                    item.source = None;
                    item.reason = Some("GPT header probe".into());
                } else if let Some(probe) = report.items.get_mut(before) {
                    probe.reason = Some("GPT header probe; superseded by primary capture".into());
                }
            }
            Err(e) => {
                item.reason = Some("partial capture retained".into());
                item.fail_source(Status::ExecutionFailed, e);
                report.unusable |= !e.is::<ReadRejected>();
            }
        }
        report.items.push(item);
        result
    });
    let result = result.and_then(|table| {
        let file = file.take().expect("capture file after GPT reads");
        file.sync_all()?;
        drop(file);
        check_bound_directory(parent, job)?;
        publish(&partial, path, overwrite)?;
        report.outputs.retain(|output| output.path != partial);
        report.output(path.to_owned(), "gpt", OutputState::Complete);
        Ok(table)
    });
    if let Err(e) = &result
        && !e.is::<Stopped>()
        && report.errors.len() == errors_before
        && report.items[before..]
            .iter()
            .all(|item| item.errors.is_empty())
    {
        report.error_source(Status::ExecutionFailed, e);
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
    let selection = PartitionSelection {
        luns: luns.to_vec(),
        partitions: PartitionTargets::from_names(None, names),
    };
    directory_controlled(channel, luns, &selection, output, overwrite, None, report);
}

struct LunOutput {
    lun: u8,
    dir: PathBuf,
    gpt: PathBuf,
}

impl LunOutput {
    fn new(root: &Path, lun: u8) -> Result<Self> {
        let dir = resolve_directory(&root.join(format!("lun{lun}")))?;
        let gpt = resolve_directory(&dir.join(".gpt"))?;
        Ok(Self { lun, dir, gpt })
    }
}

fn check_bound_directory(directory: &Path, job: Option<&Job>) -> Result<()> {
    if job.is_some() {
        ensure!(
            resolve_directory(directory)? == directory,
            "backup directory binding changed: {}",
            directory.display()
        );
    }
    Ok(())
}

pub(crate) fn directory_controlled(
    channel: &mut impl QdlChan,
    luns: &[u8],
    selection: &PartitionSelection,
    output: &Path,
    overwrite: bool,
    job: Option<&Job>,
    report: &mut Report,
) {
    report.coverage.limited = true;
    report.coverage.reason = Some(format!(
        "requested LUNs {luns:?}; per-read results define scanned scope; only listed GPT partitions"
    ));
    if check_start(job).is_err() {
        return;
    }
    let destinations = luns
        .iter()
        .map(|&lun| LunOutput::new(output, lun))
        .collect::<Result<Vec<_>>>();
    let destinations = match destinations {
        Ok(destinations) => destinations,
        Err(error) => {
            report.error_source(Status::InputError, &error);
            return;
        }
    };
    let mut directories = HashSet::new();
    let mut paths = Vec::new();
    for directory in destinations
        .iter()
        .flat_map(|output| [&output.dir, &output.gpt])
    {
        if !directories.insert(directory) {
            report.error(
                Status::InputError,
                format!(
                    "backup destinations share a directory: {}",
                    directory.display()
                ),
            );
            return;
        }
        paths.push(PathUse::bound(directory.clone(), true));
    }
    if let Some(job) = job {
        if let Err(error) = job.bind_paths(paths) {
            report.error_source(Status::InputError, &error);
            return;
        }
        if check_dispatch(Some(job), "backup destinations bound").is_err() {
            return;
        }
    }
    let mut found = HashSet::new();
    let mut complete = true;
    for destination in &destinations {
        if !backup_lun(
            channel,
            destination,
            selection,
            overwrite,
            job,
            report,
            &mut found,
        ) {
            complete = false;
            break;
        }
    }
    for selection in selection.partitions.explicit() {
        if complete
            && !found.iter().any(|(lun, name)| {
                selection.name == *name && selection.lun.is_none_or(|n| n == *lun)
            })
        {
            report.error(
                Status::InputError,
                format!(
                    "selected partition not found in scanned scope: {}",
                    selection.name
                ),
            );
        }
    }
    report.summarize();
}

fn backup_lun(
    channel: &mut impl QdlChan,
    output: &LunOutput,
    selection: &PartitionSelection,
    overwrite: bool,
    job: Option<&Job>,
    report: &mut Report,
    found: &mut HashSet<(u8, String)>,
) -> bool {
    if check_start(job).is_err() {
        return false;
    }
    let LunOutput { lun, dir, gpt } = output;
    let lun = *lun;
    for directory in [dir, gpt] {
        if let Err(error) = check_bound_directory(directory, job) {
            report.error_source(Status::InputError, &error);
            return false;
        }
    }
    let primary_path = gpt.join("primary.bin");
    let backup_path = gpt.join("backup.bin");
    for path in [&primary_path, &backup_path] {
        let vacant = if overwrite {
            Ok(())
        } else {
            require_vacant(path)
        };
        if let Err(error) = vacant.and_then(|()| require_vacant(&partial_path(path))) {
            report.error_source(Status::InputError, &error);
            return false;
        }
    }
    let table = match capture_primary(channel, lun, &primary_path, overwrite, job, report) {
        Ok(table) => table,
        Err(_) => {
            report.summarize();
            return false;
        }
    };
    if let Some(job) = job
        && let Err(error) = job.check_gpt(lun, &table)
    {
        report.error_source(Status::InputError, &error);
        return false;
    }
    let selected = storage::partitions(&table, lun).expect("validated GPT");
    let selected: Vec<_> = selected
        .into_iter()
        .filter(|p| selection.partitions.includes(p.lun, &p.name))
        .collect();
    for p in &selected {
        found.insert((p.lun, p.name.clone()));
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
            item.reject(
                Status::Unsupported,
                "partition name is unsafe or conflicts within selected LUN",
            );
            report.items.push(item);
        }
        return true;
    }
    if check_start(job).is_err() {
        return false;
    }
    let h = &table.gpt.header;
    let backup_start = h.last_usable_lba + 1;
    let backup_sectors = h.backup_lba - h.last_usable_lba;
    let sector = table.gpt.sector_size;
    let mut backup = Item::read(lun, None, backup_start * sector, backup_sectors * sector);
    backup.partition_bytes = None;
    backup.source = Some(backup_path.clone());
    // The metadata span is bounded independently of the disk size.
    let mut publish_failed = false;
    if backup_sectors * sector > 16 * 1024 * 1024 {
        backup.reject(Status::Unsupported, "backup GPT capture exceeds 16 MiB");
    } else {
        let mut progress = CaptureProgress::default();
        let result = save_capture(
            &backup_path,
            overwrite,
            &mut backup,
            &mut progress,
            job,
            |mut out| {
                crate::firehose::read_storage(
                    channel,
                    &mut out,
                    usize::try_from(backup_sectors)?,
                    0,
                    lun,
                    backup_start,
                )
            },
        );
        if let Err(error) = &result {
            if error.is::<Stopped>() {
                progress.record(report, &backup_path, "gpt", &result);
                return false;
            } else if progress.ack {
                backup.execution = ExecutionState::Done;
                report.error_source(Status::ExecutionFailed, error);
                publish_failed = true;
            } else {
                backup.fail_source(Status::ExecutionFailed, error);
            }
        }
        progress.record(report, &backup_path, "gpt", &result);
    }
    let read_failed = "previous read failed; session stopped";
    let mut stopped = (!backup.errors.is_empty()).then_some(read_failed);
    report.items.push(backup);
    if publish_failed {
        return false;
    }
    for p in &selected {
        if check_start(job).is_err() {
            return false;
        }
        let partition_path = dir.join(format!("{}.bin", p.name));
        let mut item = Item::read(
            lun,
            Some(p.name.clone()),
            p.start_byte().expect("validated GPT"),
            p.bytes().expect("validated GPT"),
        );
        if !storage::safe_name(&p.name) || storage::name_conflicts(&selected, p) {
            item.reject(
                Status::Unsupported,
                "partition name is unsafe or conflicts within selected LUN",
            );
        } else {
            item.source = Some(partition_path.clone());
            if let Some(reason) = stopped {
                item.reason = Some(reason.into());
            } else if let Err(error) = check_bound_directory(dir, job) {
                item.reject(Status::InputError, format!("{error:#}"));
                report.items.push(item);
                return false;
            } else {
                let mut progress = CaptureProgress::default();
                let result = save_capture(
                    &partition_path,
                    overwrite,
                    &mut item,
                    &mut progress,
                    job,
                    |mut out| storage::read_partition(channel, p, &mut out),
                );
                if let Err(e) = &result {
                    if e.is::<Stopped>() {
                        progress.record(report, &partition_path, "partition", &result);
                        return false;
                    }
                    if e.is::<OutputCollision>() {
                        item.reject(Status::InputError, e.to_string());
                    } else if progress.ack {
                        item.execution = ExecutionState::Done;
                        report.error_source(Status::ExecutionFailed, e);
                        stopped = Some(
                            "previous backup publication failed; remaining reads not attempted",
                        );
                    } else {
                        item.fail_source(Status::ExecutionFailed, e);
                        stopped = Some(read_failed);
                    }
                }
                progress.record(report, &partition_path, "partition", &result);
            }
        }
        report.items.push(item);
    }
    report.summarize();
    stopped.is_none()
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
