use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

/// Reject special inputs before opening; recheck the opened handle for replacement races.
pub(crate) fn open_regular(path: &Path) -> Result<File> {
    if !fs::metadata(path)?.is_file() {
        bail!("input is not a regular file: {}", path.display());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular file replaced by a FIFO must not block between the two checks.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        bail!("input is not a regular file: {}", path.display());
    }
    Ok(file)
}

#[derive(Debug)]
pub(crate) struct Changed(pub PathBuf);

impl std::fmt::Display for Changed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "source changed or became unavailable: {}",
            self.0.display()
        )
    }
}

impl std::error::Error for Changed {}

#[derive(Clone, PartialEq, Eq)]
struct Fingerprint {
    length: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(windows)]
    identity: Option<(u32, u64)>,
}

impl Fingerprint {
    fn new(metadata: Metadata, #[cfg(windows)] file: &File) -> Result<Self> {
        anyhow::ensure!(metadata.is_file(), "source is not a regular file");
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
            #[cfg(windows)]
            identity: windows_identity(file),
        })
    }

    fn from_handle(file: &File) -> Result<Self> {
        Self::new(
            file.metadata()?,
            #[cfg(windows)]
            file,
        )
    }

    fn from_path(path: &Path) -> Result<Self> {
        #[cfg(windows)]
        return Self::from_handle(&open_regular(path)?);
        #[cfg(not(windows))]
        Self::new(fs::metadata(path)?)
    }
}

#[cfg(windows)]
fn windows_identity(file: &File) -> Option<(u32, u64)> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;
    let mut info = MaybeUninit::uninit();
    // The API initializes the structure when it returns success.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

/// Clones share an open file on the operation worker. Each reader seeks before use.
#[derive(Clone)]
pub(crate) struct SourceFile(Arc<File>);

impl SourceFile {
    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        self.0.metadata()
    }
}

impl Read for SourceFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Seek for SourceFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        (&*self.0).seek(pos)
    }
}

struct Source {
    file: SourceFile,
    fingerprint: Fingerprint,
}

impl Source {
    fn check(&self, path: &Path) -> Result<()> {
        if Fingerprint::from_path(path).ok().as_ref() != Some(&self.fingerprint)
            || Fingerprint::from_handle(&self.file.0).ok().as_ref() != Some(&self.fingerprint)
        {
            return Err(Changed(path.to_owned()).into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Sources {
    files: BTreeMap<PathBuf, Source>,
    absent: BTreeSet<PathBuf>,
    directories: BTreeMap<PathBuf, PathBuf>,
}

impl Sources {
    pub(crate) fn directory(&mut self, path: &Path) -> Result<PathBuf> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("open {}", path.display()))?;
        anyhow::ensure!(canonical.is_dir(), "not a directory: {}", path.display());
        self.directories.insert(path.to_owned(), canonical.clone());
        Ok(canonical)
    }

    pub(crate) fn open(&mut self, path: &Path) -> Result<SourceFile> {
        use std::collections::btree_map::Entry;
        let source = match self.files.entry(path.to_owned()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let file =
                    open_regular(path).with_context(|| format!("open {}", path.display()))?;
                let fingerprint = Fingerprint::from_handle(&file)?;
                entry.insert(Source {
                    file: SourceFile(Arc::new(file)),
                    fingerprint,
                })
            }
        };
        source.check(path)?;
        let mut file = source.file.clone();
        file.rewind()?;
        Ok(file)
    }

    pub(crate) fn expect_absent(&mut self, path: PathBuf) -> Result<()> {
        ensure_absent(&path)?;
        self.absent.insert(path);
        Ok(())
    }

    pub(crate) fn check(&self) -> Result<()> {
        for (path, canonical) in &self.directories {
            if path.canonicalize().ok().as_ref() != Some(canonical) || !canonical.is_dir() {
                return Err(Changed(path.clone()).into());
            }
        }
        for (path, source) in &self.files {
            source.check(path)?;
        }
        for path in &self.absent {
            ensure_absent(path)?;
        }
        Ok(())
    }
}

fn ensure_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(Changed(path.to_owned()).into()),
    }
}

/// Path bindings for the two active work slots. Resolve existing aliases off the UI thread.
pub(crate) struct PathUse {
    path: PathBuf,
    writes: bool,
}

impl PathUse {
    pub(crate) fn new(path: &Path, writes: bool) -> Self {
        Self::bound(resolved_binding(path), writes)
    }

    pub(crate) fn bound(path: PathBuf, writes: bool) -> Self {
        Self { path, writes }
    }

    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        (self.writes || other.writes)
            && (self.path.starts_with(&other.path) || other.path.starts_with(&self.path))
    }
}

/// Resolve a destination against its nearest existing directory.
/// Missing components are allowed. Dangling links and non-directories fail.
pub(crate) fn resolve_directory(path: &Path) -> Result<PathBuf> {
    let mut base = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    let mut missing = Vec::new();
    loop {
        match base.canonicalize() {
            Ok(mut resolved) => {
                anyhow::ensure!(resolved.is_dir(), "not a directory: {}", base.display());
                resolved.extend(missing.into_iter().rev());
                return Ok(resolved);
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && fs::symlink_metadata(base)
                        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound) =>
            {
                let name = base.file_name().context("missing directory name")?;
                missing.push(name);
                base = base.parent().context("missing directory parent")?;
                if base.as_os_str().is_empty() {
                    base = Path::new(".");
                }
            }
            Err(error) => {
                return Err(error).with_context(|| format!("resolve directory {}", base.display()));
            }
        }
    }
}

#[cfg(feature = "cloud")]
pub(crate) fn check_path_binding(
    original: &Path,
    actual: Result<PathBuf>,
    expected: &Path,
) -> Result<()> {
    if actual.as_deref().ok() != Some(expected) {
        return Err(Changed(original.to_owned()).into());
    }
    Ok(())
}

#[cfg(feature = "cloud")]
pub(crate) fn parse_private_json<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    phase: &str,
) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|error| {
        anyhow::anyhow!(
            "{phase} at line {}, column {} ({:?})",
            error.line(),
            error.column(),
            error.classify()
        )
    })
}

fn resolved_binding(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut base = path.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(mut canonical) = base.canonicalize() {
            canonical.extend(tail.into_iter().rev());
            return canonical;
        }
        if let (Some(parent), Some(name)) = (base.parent(), base.file_name()) {
            tail.push(name);
            base = parent;
        } else {
            return path;
        }
    }
}

pub(crate) fn request_paths(request: &crate::model::Request) -> Vec<PathUse> {
    use crate::model::{
        Connection, FlashInput, LoaderInput, OperationRequest, PackageArtifacts, Request,
    };

    let mut paths = Vec::new();
    let mut bind = |path: &Path, writes| {
        if !path.as_os_str().is_empty() {
            paths.push(PathUse::new(path, writes));
        }
    };
    let (loader, operation) = match request {
        Request::Connect(config) => (Some(&config.loader), None),
        Request::Run {
            connection,
            operation,
        } => {
            let loader = match connection {
                Connection::Open(config) => Some(&config.loader),
                _ => None,
            };
            (loader, Some(operation))
        }
        _ => return paths,
    };
    let mut cloud = None;
    match loader {
        Some(LoaderInput::Local {
            loader,
            signed_digest,
            digest_signature,
        }) => {
            for path in [loader, signed_digest, digest_signature]
                .into_iter()
                .flatten()
            {
                bind(path, false);
            }
        }
        Some(LoaderInput::Cloud(input)) => cloud = Some((input, true)),
        None => {}
    }
    match operation {
        Some(OperationRequest::Backup { output, .. }) => bind(output, true),
        Some(OperationRequest::Restore { from, image, .. }) => {
            for path in [from, image].into_iter().flatten() {
                bind(path, false);
            }
        }
        Some(OperationRequest::Flash {
            input: FlashInput::Xml {
                programs, patches, ..
            },
            ..
        }) => {
            for path in programs.iter().chain(patches) {
                bind(path, false);
            }
        }
        Some(OperationRequest::Flash {
            input: FlashInput::Package { input, artifacts },
            ..
        }) => {
            bind(&input.root, false);
            if let Some(config) = &input.config {
                bind(config, false);
            }
            match artifacts {
                PackageArtifacts::Generate(output) => bind(output, true),
                PackageArtifacts::Reopen(output) => bind(output, false),
            }
        }
        Some(OperationRequest::PreparePackage { input, output }) => {
            bind(&input.root, false);
            if let Some(config) = &input.config {
                bind(config, false);
            }
            bind(output, true);
        }
        Some(OperationRequest::CloudFetch(input)) => cloud = Some((input, true)),
        Some(OperationRequest::CloudIdentity {
            input,
            confirmed_path,
        }) => {
            cloud = Some((input, false));
            if let Some(path) = confirmed_path {
                bind(path, true);
            }
        }
        _ => {}
    }
    if let Some((input, fetch)) = cloud {
        for (path, writes) in [(&input.config, false), (&input.output, true)] {
            if writes && !fetch {
                continue;
            }
            if path.is_absolute() {
                bind(path, writes);
            } else if let Some(base) = &input.base_directory {
                bind(&base.join(path), writes);
            }
        }
    }
    paths
}
