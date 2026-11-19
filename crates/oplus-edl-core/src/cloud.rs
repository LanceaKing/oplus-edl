//! Anteasy preview 4.2026.009 cloud-loader transport, recovered with Binary Ninja.
//! SWM registration obtains enrollment tickets automatically; manual credentials remain optional.
use crate::{
    host::HostServices,
    input::{PathUse, check_path_binding, open_regular, parse_private_json, resolve_directory},
    model::{CloudInput, IdentityMode},
    result::{OutputState, Report},
    service::Job,
    session::{check_dispatch, check_start},
};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;
use wreq::Client;
const ORIGIN: &str = "https://oplustool.anteasy.com";
const MAX_ASSET: usize = 64 * 1024 * 1024;
#[derive(Deserialize)]
pub struct Config {
    pub swm: Option<crate::swm::Config>,
    pub enrollment_ticket: Option<String>,
    pub credentials: Option<Credentials>,
    pub query: Query,
    pub profile_id: Option<u64>,
}
#[derive(Deserialize)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
    pub delegation_id: String,
    pub expires_at: u64,
}
#[derive(Deserialize, Serialize)]
pub struct Query {
    pub platform: String,
    pub device_code: String,
    pub scheme: String,
    pub pk_hash_sha256: String,
    pub pk_hash_sha384: String,
    pub msm_id: String,
}
#[derive(Deserialize)]
struct List {
    items: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    id: u64,
    platform: String,
    scheme: String,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    id: u64,
    file_type: String,
}
#[derive(Deserialize, Serialize)]
struct Ticket {
    success: bool,
    manifest_version: String,
    client_id: String,
    profile_id: u64,
    asset_id: u64,
    file_type: String,
    size: u64,
    checksum_sha256: String,
    expires_at_unix: u64,
    download_url: String,
    download_url_sha256: String,
    manifest_signature: String,
}
pub type Material = crate::result::CloudAssets;
// Explicit legacy tickets/credentials have no authenticated time anchor and retain
// host-clock semantics. Automatic enrollment carries the SWM signed clock instead.
enum Clock {
    Host,
    Authenticated(crate::swm::AuthenticatedClock),
}
impl Clock {
    fn now(&self) -> Result<u64> {
        self.with_host_clock(now)
    }
    fn with_host_clock(&self, host: impl FnOnce() -> Result<u64>) -> Result<u64> {
        match self {
            Self::Host => host(),
            Self::Authenticated(clock) => Ok(clock.now()),
        }
    }
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}
fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}
fn mac(key: &str, bytes: &[u8]) -> Hmac<Sha256> {
    let mut m =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts arbitrary key lengths");
    m.update(bytes);
    m
}
fn signature(key: &str, bytes: &[u8]) -> String {
    hex(&mac(key, bytes).finalize().into_bytes())
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    ensure!(
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid cloud SHA-256 encoding"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
        .collect()
}
fn valid_credentials(c: &Credentials, time: u64) -> Result<()> {
    ensure!(
        !c.client_id.is_empty()
            && c.client_id.len() <= 256
            && !c.client_secret.is_empty()
            && c.client_secret.len() <= 1024
            && !c.delegation_id.is_empty()
            && c.delegation_id.len() <= 1024,
        "invalid cloud credentials"
    );
    ensure!(
        c.expires_at > time && c.expires_at <= time.saturating_add(605),
        "cloud credentials expired or outside preview lifetime"
    );
    Ok(())
}
// loader.dll 0x180056bc0: method, path, query, body SHA256, seconds, nonce, client ID.
fn request_signature(
    c: &Credentials,
    path: &str,
    body: &[u8],
    timestamp: u64,
    nonce: &str,
) -> String {
    signature(
        &c.client_secret,
        format!(
            "POST\n{path}\n\n{}\n{timestamp}\n{nonce}\n{}",
            sha(body),
            c.client_id
        )
        .as_bytes(),
    )
}
fn validate_ticket(
    t: &Ticket,
    c: &Credentials,
    profile: u64,
    asset: u64,
    kind: &str,
    time: u64,
) -> Result<()> {
    ensure!(
        t.success && t.manifest_version == "cloud_loader_ticket_v2",
        "invalid cloud ticket version/status"
    );
    ensure!(
        t.client_id == c.client_id
            && t.profile_id == profile
            && t.asset_id == asset
            && t.file_type == kind,
        "cloud ticket binding mismatch"
    );
    ensure!(
        t.expires_at_unix > time && t.size > 0 && t.size <= MAX_ASSET as u64,
        "expired or oversized cloud ticket"
    );
    unhex(&t.checksum_sha256)?;
    unhex(&t.download_url_sha256)?;
    ensure!(
        sha(t.download_url.as_bytes()).eq_ignore_ascii_case(&t.download_url_sha256),
        "cloud download URL checksum mismatch"
    );
    let url = Url::parse(&t.download_url).context("invalid cloud download URL")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && t.download_url.len() <= 4096,
        "unsupported cloud download URL"
    );
    // loader.dll 0x180044cf0; final field has no trailing newline.
    let text = format!(
        "cloud_loader_ticket_v2\nclient_id:{}\nprofile_id:{}\nasset_id:{}\nfile_type:{}\nsize:{}\nsha256:{}\nexpires_at:{}\ndownload_url_sha256:{}",
        t.client_id,
        t.profile_id,
        t.asset_id,
        t.file_type,
        t.size,
        t.checksum_sha256,
        t.expires_at_unix,
        t.download_url_sha256
    );
    mac(&c.client_secret, text.as_bytes())
        .verify_slice(&unhex(&t.manifest_signature)?)
        .map_err(|_| anyhow::anyhow!("cloud ticket HMAC mismatch"))?;
    Ok(())
}
async fn bounded(response: wreq::Response, limit: usize) -> Result<Vec<u8>> {
    let status = response.status();
    if let Some(size) = response.content_length() {
        ensure!(size <= limit as u64, "cloud body exceeds limit");
    }
    let mut stream = response.bytes_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow::anyhow!("cloud response stream failed"))?;
        ensure!(
            chunk.len() <= limit - data.len(),
            "cloud body exceeds limit"
        );
        data.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        anyhow::bail!("cloud HTTP status {}", status.as_u16());
    }
    Ok(data)
}
async fn post(
    client: &Client,
    origin: &str,
    path: &str,
    body: Vec<u8>,
    credentials: Option<&Credentials>,
    clock: &Clock,
) -> Result<Vec<u8>> {
    let mut request = client
        .post(format!("{origin}{path}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json");
    if let Some(c) = credentials {
        let time = clock.now()?;
        valid_credentials(c, time)?;
        let nonce = hex(&rand::random::<[u8; 16]>());
        request = request
            .header("X-Client-ID", &c.client_id)
            .header("X-Timestamp", time.to_string())
            .header("X-Nonce", &nonce)
            .header("X-Signature-Version", "1")
            .header(
                "X-Signature",
                request_signature(c, path, &body, time, &nonce),
            );
    }
    let response = request
        .body(body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("cloud API request failed: {path}"))?;
    bounded(response, 4 * 1024 * 1024)
        .await
        .with_context(|| format!("cloud API {path}"))
}
async fn fetch(
    config: Config,
    output: &Path,
    origin: &str,
    mode: IdentityMode,
    host: &HostServices,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<Material> {
    let check = || check_dispatch(job, "cloud acquisition");
    check()?;
    ensure!(
        config.query.platform == "qualcomm" && config.query.scheme == "DIGEST",
        "cloud flash currently requires qualcomm DIGEST"
    );
    ensure!(
        config.query.msm_id.len() == 8
            && config.query.msm_id.bytes().all(|b| b.is_ascii_hexdigit()),
        "cloud query requires an observed eight-digit MSM ID"
    );
    for (s, len) in [
        (&config.query.pk_hash_sha256, 64),
        (&config.query.pk_hash_sha384, 96),
    ] {
        ensure!(
            s.is_empty() || s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid cloud public key hash"
        );
    }
    ensure!(
        usize::from(config.swm.is_some())
            + usize::from(config.enrollment_ticket.is_some())
            + usize::from(config.credentials.is_some())
            == 1,
        "provide swm configuration, or one explicit legacy credential source"
    );
    ensure!(
        !output.try_exists()?,
        "cloud output must be a new directory"
    );
    let client = Client::builder()
        .redirect(wreq::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120));
    let client = if origin == ORIGIN {
        client
    } else {
        client.no_proxy()
    }
    .build()?;
    let mut clock = Clock::Host;
    let credentials = match config.credentials {
        Some(c) => c,
        None => {
            let ticket = match config.swm {
                Some(swm) => {
                    let (ticket, authenticated) =
                        crate::swm::enrollment_with_clock(&swm, mode, &check, host, report)
                            .await
                            .context("automatic SWM enrollment")?;
                    clock = Clock::Authenticated(authenticated);
                    ticket
                }
                None => config
                    .enrollment_ticket
                    .context("missing enrollment source")?,
            };
            ensure!(
                !ticket.is_empty() && ticket.len() <= 256 * 1024,
                "invalid enrollment ticket length"
            );
            let body = serde_json::to_vec(
                &serde_json::json!({"ticket":ticket,"app_id":"abd975ae-a061-4137-a0c2-90ebdf17d3f2","release_id":"df052167-1c7e-4e68-a32b-0dd485f1be41","app_version":"4.2026.009"}),
            )?;
            check()?;
            parse_private_json::<Credentials>(
                &post(
                    &client,
                    origin,
                    "/api/v1/cloud-loader/enroll",
                    body,
                    None,
                    &clock,
                )
                .await?,
                "invalid cloud enrollment response",
            )?
        }
    };
    valid_credentials(&credentials, clock.now()?)?;
    check()?;
    let response = post(
        &client,
        origin,
        "/api/v1/cloud-loader/list",
        serde_json::to_vec(&config.query)?,
        Some(&credentials),
        &clock,
    )
    .await?;
    let list: List = parse_private_json(&response, "invalid cloud list response")?;
    let offered = list
        .items
        .iter()
        .map(|p| format!("{}:{}/{}", p.id, p.platform, p.scheme))
        .collect::<Vec<_>>()
        .join(", ");
    let matches: Vec<_> = list
        .items
        .into_iter()
        .filter(|p| {
            p.platform.eq_ignore_ascii_case(&config.query.platform)
                && p.scheme.eq_ignore_ascii_case(&config.query.scheme)
                && config.profile_id.is_none_or(|id| id == p.id)
        })
        .collect();
    ensure!(
        matches.len() == 1,
        "cloud profile selection requires exactly one match; got {}; offered [{offered}] (set profile_id to disambiguate)",
        matches.len()
    );
    let profile = &matches[0];
    let mut files = Vec::new();
    let mut evidence = Vec::new();
    for kind in ["loader", "digest", "sign"] {
        check()?;
        let assets: Vec<_> = profile
            .assets
            .iter()
            .filter(|a| a.file_type == kind)
            .collect();
        ensure!(assets.len() == 1, "cloud profile requires one {kind} asset");
        let asset = assets[0];
        let body = serde_json::to_vec(
            &serde_json::json!({"profile_id":profile.id,"asset_id":asset.id,"file_type":kind}),
        )?;
        let reply = post(
            &client,
            origin,
            "/api/v1/cloud-loader/ticket",
            body,
            Some(&credentials),
            &clock,
        )
        .await?;
        let ticket: Ticket = parse_private_json(&reply, "invalid cloud ticket response")?;
        validate_ticket(
            &ticket,
            &credentials,
            profile.id,
            asset.id,
            kind,
            clock.now()?,
        )?;
        // The signed HTTPS URL is a separate asset request. Never forward API credentials.
        check()?;
        let response = client
            .get(&ticket.download_url)
            .header("Accept", "application/octet-stream")
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("cloud {kind} download request failed"))?;
        let data = bounded(response, ticket.size as usize).await?;
        ensure!(
            data.len() as u64 == ticket.size
                && sha(&data).eq_ignore_ascii_case(&ticket.checksum_sha256),
            "cloud {kind} size or checksum mismatch"
        );
        if kind == "loader" {
            ensure!(
                data.starts_with(b"\x7fELF"),
                "cloud Qualcomm loader is not ELF"
            );
        } else {
            ensure!(
                data.len() <= 16 * 1024 * 1024,
                "cloud authentication input too large"
            );
        }
        evidence.push(serde_json::json!({"profile_id":profile.id,"asset_id":asset.id,"file_type":kind,"size":data.len(),"sha256":sha(&data)}));
        files.push(data);
    }
    // Publish only after all three files passed ticket and payload checks.
    let manifest = serde_json::json!({"source":"Anteasy cloud-loader","query":config.query,"assets":evidence,"device_tested":false});
    publish_assets(output, files, &manifest, profile.id, job, report)
}

fn publish_assets(
    output: &Path,
    files: Vec<Vec<u8>>,
    evidence: &serde_json::Value,
    profile_id: u64,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<Material> {
    let check_output = |step| {
        check_dispatch(job, step)?;
        check_path_binding(output, resolve_directory(output), output)?;
        check_start(job)
    };
    check_output("cloud output directory")?;
    fs::create_dir(output)?;
    let paths = [
        output.join("loader.elf"),
        output.join("digest.bin"),
        output.join("sign.bin"),
    ];
    for ((path, kind), data) in paths
        .iter()
        .zip(["loader", "signed_digest", "digest_signature"])
        .zip(files)
    {
        check_output(kind)?;
        let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
        report.output(path.clone(), kind, OutputState::Partial);
        f.write_all(&data)?;
        f.sync_all()?;
        report.output(path.clone(), kind, OutputState::Complete);
    }
    check_output("asset manifest")?;
    let manifest = output.join("assets.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest)?;
    report.output(manifest.clone(), "asset_manifest", OutputState::Partial);
    serde_json::to_writer_pretty(&mut file, evidence)?;
    file.sync_all()?;
    report.output(manifest.clone(), "asset_manifest", OutputState::Complete);
    let [loader, digest, signature] = paths;
    Ok(Material {
        loader,
        digest,
        signature,
        manifest,
        profile_id,
    })
}
pub fn download(config: &Path, output: &Path) -> Result<Material> {
    let input = CloudInput {
        config: config.into(),
        output: output.into(),
        base_directory: Some(std::env::current_dir()?),
        identity_mode: IdentityMode::OpenExisting,
    };
    download_controlled(
        &input,
        &HostServices::default(),
        None,
        &mut Report::new(None, None),
    )
}

fn absolute_path(input: &CloudInput, path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "missing cloud path");
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    let base = input
        .base_directory
        .as_ref()
        .filter(|base| base.is_absolute())
        .context("select an absolute base directory for relative cloud paths")?;
    let resolved = base.join(path);
    ensure!(
        resolved.is_absolute(),
        "cloud path does not resolve to an absolute path"
    );
    Ok(resolved)
}

#[cfg(test)]
pub(crate) fn resolve_config(input: &CloudInput) -> Result<Config> {
    read_config(input, &absolute_path(input, &input.config)?)
}

fn read_config(input: &CloudInput, path: &Path) -> Result<Config> {
    let f = open_regular(path)?;
    let mut bytes = Vec::new();
    f.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "cloud config too large");
    let mut config: Config = parse_private_json(&bytes, "invalid cloud configuration")?;
    if let Some(swm) = &mut config.swm {
        swm.identity_path = absolute_path(input, &swm.identity_path)?;
        swm.release_directory = absolute_path(input, &swm.release_directory)?;
    } else {
        ensure!(
            input.identity_mode == IdentityMode::OpenExisting,
            "--create-identity requires SWM configuration"
        );
    }
    Ok(config)
}

fn bind_cloud_paths(
    input: &CloudInput,
    with_output: bool,
    job: Option<&Job>,
) -> Result<(PathBuf, Option<PathBuf>)> {
    let config = absolute_path(input, &input.config)?;
    let output = with_output
        .then(|| absolute_path(input, &input.output))
        .transpose()?;
    let actual_config = config
        .canonicalize()
        .with_context(|| format!("resolve cloud configuration {}", config.display()))?;
    let actual_output = output.as_deref().map(resolve_directory).transpose()?;
    if let Some(job) = job {
        let mut paths = vec![PathUse::bound(actual_config.clone(), false)];
        if let Some(path) = &actual_output {
            paths.push(PathUse::bound(path.clone(), true));
        }
        job.bind_paths(paths)?;
        check_dispatch(Some(job), "cloud paths bound")?;
        check_path_binding(
            &config,
            config.canonicalize().map_err(Into::into),
            &actual_config,
        )?;
        if let (Some(original), Some(actual)) = (&output, &actual_output) {
            check_path_binding(original, resolve_directory(original), actual)?;
        }
    }
    check_start(job)?;
    Ok((actual_config, actual_output))
}

pub(crate) fn download_controlled(
    input: &CloudInput,
    host: &HostServices,
    job: Option<&Job>,
    report: &mut Report,
) -> Result<Material> {
    check_dispatch(job, "cloud acquisition")?;
    let (config_path, output) = bind_cloud_paths(input, true, job)?;
    let mut config = read_config(input, &config_path)?;
    let output = output.expect("bound asset destination");
    let loopback = host.cloud_loopback();
    ensure!(
        loopback.is_none() || config.swm.is_none(),
        "SWM configuration is unavailable in the loopback environment"
    );
    if let Some(swm) = config.swm.take() {
        let bound = swm.resolve(input.identity_mode)?;
        if let Some(job) = job {
            bind_swm(job, &swm, &bound, input.identity_mode)?;
            if input.identity_mode == IdentityMode::CreateNew {
                job.log(
                    "acquisition",
                    &format!("Create SWM identity at {}", bound.identity_path.display()),
                );
            }
        }
        config.swm = Some(bound);
    }
    #[cfg(test)]
    if host.authenticated_asset_fixture {
        return publish_assets(
            &output,
            vec![b"\x7fELF".to_vec(), vec![2], vec![3]],
            &serde_json::json!({}),
            11,
            job,
            report,
        );
    }
    let origin = loopback.map(|address| format!("http://{address}"));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fetch(
            config,
            &output,
            origin.as_deref().unwrap_or(ORIGIN),
            input.identity_mode,
            host,
            job,
            report,
        ))
}

fn bind_swm(
    job: &Job,
    swm: &crate::swm::Config,
    bound: &crate::swm::Config,
    mode: IdentityMode,
) -> Result<()> {
    job.bind_paths(vec![
        PathUse::bound(bound.identity_path.clone(), mode == IdentityMode::CreateNew),
        PathUse::bound(bound.release_directory.clone(), false),
    ])?;
    check_dispatch(Some(job), "SWM inputs bound")?;
    swm.check_binding(bound, mode)
}

pub(crate) fn installation_identity(
    input: &CloudInput,
    confirmed: Option<&Path>,
    loopback: Option<std::net::SocketAddr>,
    job: &Job,
    report: &mut Report,
) -> Result<PathBuf> {
    let check = || crate::session::check_start(Some(job));
    check()?;
    ensure!(
        loopback.is_none(),
        "SWM configuration is unavailable in the loopback environment"
    );
    let (config, _) = bind_cloud_paths(input, false, Some(job))?;
    let swm = read_config(input, &config)?
        .swm
        .context("an installation identity requires SWM configuration")?;
    let mode = IdentityMode::CreateNew;
    let identity = swm.resolve_identity(mode);
    let identity_path = if let Some(path) = confirmed {
        check_path_binding(path, identity, path)
            .context("cloud identity location changed; review it again")?;
        path.to_owned()
    } else {
        identity?
    };
    let bound = crate::swm::Config {
        identity_path,
        release_directory: resolve_directory(&swm.release_directory)?,
    };
    if confirmed.is_some() {
        bind_swm(job, &swm, &bound, mode)?;
        crate::swm::create_identity(&bound, &check, report)?;
    }
    Ok(bound.identity_path)
}

#[cfg(test)]
#[path = "../tests/support/cloud_fixture.rs"]
mod cloud_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn publication_keeps_its_bound_asset_path_after_the_caller_alias_moves() {
        use crate::{Connection, CoreService, OperationRequest, Request, result::Status};
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let actual = root.join("original");
        let other = root.join("replacement");
        let alias = root.join("alias");
        let config = root.join("cloud.json");
        fs::create_dir(&actual).unwrap();
        fs::create_dir(&other).unwrap();
        symlink(&actual, &alias).unwrap();
        fs::write(other.join("retained"), b"sentinel").unwrap();
        cloud_fixture::write_config(&config).unwrap();
        let mut host = HostServices::default();
        host.authenticated_asset_fixture = true;
        let service = CoreService::new(host);
        let output = alias.join("assets");
        service.set_dispatch_gate(
            "cloud output directory",
            Box::new({
                let other = other.clone();
                move || {
                    fs::remove_file(&alias).unwrap();
                    symlink(other, alias).unwrap();
                }
            }),
        );
        let handle = service
            .submit(Request::Run {
                connection: Connection::Offline,
                operation: OperationRequest::CloudFetch(CloudInput {
                    config,
                    output,
                    base_directory: None,
                    identity_mode: IdentityMode::OpenExisting,
                }),
            })
            .unwrap();
        let start = std::time::Instant::now();
        let result = loop {
            if let Some(result) = handle.try_take_result() {
                break result;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(result.status, Status::Success, "{result:?}");
        assert_eq!(result.outputs.len(), 4);
        assert!(
            result
                .outputs
                .iter()
                .all(|output| output.path.starts_with(actual.join("assets"))
                    && output.path.is_file())
        );
        assert!(!other.join("assets").exists());
        assert_eq!(fs::read(other.join("retained")).unwrap(), b"sentinel");
    }

    #[test]
    fn service_preserves_cloud_files_after_publication_error_or_late_stop() {
        use crate::{
            Connection, CoreService, OperationRequest, Request, StopReason, result::Status,
        };
        use std::{sync::mpsc, thread, time::Instant};

        let limit = Duration::from_secs(5);
        for collision in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().canonicalize().unwrap();
            let config = root.join("cloud.json");
            let output = root.join("assets");
            cloud_fixture::write_config(&config).unwrap();
            let mut host = HostServices::default();
            host.authenticated_asset_fixture = true;
            let service = CoreService::new(host);
            let (entered, boundary) = mpsc::channel();
            let (release, pending) = mpsc::channel();
            if collision {
                let conflicting = output.join("digest.bin");
                service.set_dispatch_gate(
                    "signed_digest",
                    Box::new(move || {
                        fs::write(conflicting, b"other creator").unwrap();
                    }),
                );
            } else {
                service.set_dispatch_gate(
                    "terminal commit",
                    Box::new(move || {
                        entered.send(()).unwrap();
                        pending.recv_timeout(limit).unwrap();
                    }),
                );
            }
            let handle = service
                .submit(Request::Run {
                    connection: Connection::Offline,
                    operation: OperationRequest::CloudFetch(CloudInput {
                        config,
                        output: output.clone(),
                        base_directory: None,
                        identity_mode: IdentityMode::OpenExisting,
                    }),
                })
                .unwrap();
            if !collision {
                boundary.recv_timeout(limit).unwrap();
                assert!(output.join("assets.json").is_file());
                handle.request_stop();
                release.send(()).unwrap();
            }
            let start = Instant::now();
            let result = loop {
                if let Some(result) = handle.try_take_result() {
                    break result;
                }
                assert!(start.elapsed() < limit);
                thread::sleep(Duration::from_millis(1));
            };
            let paths: &[&str] = if collision {
                &["loader.elf"]
            } else {
                &["loader.elf", "digest.bin", "sign.bin", "assets.json"]
            };
            assert_eq!(result.outputs.len(), paths.len());
            for (output_file, name) in result.outputs.iter().zip(paths) {
                assert_eq!(output_file.path, output.join(name));
                assert_eq!(output_file.state, OutputState::Complete);
            }
            assert!(result.data.is_none());
            if collision {
                assert_eq!(result.status, Status::InputError);
                assert_eq!(
                    result.errors[0].io_kind,
                    Some(std::io::ErrorKind::AlreadyExists)
                );
                assert_eq!(
                    fs::read(output.join("digest.bin")).unwrap(),
                    b"other creator"
                );
                assert!(!output.join("assets.json").exists());
            } else {
                assert_eq!(result.status, Status::Stopped);
                assert_eq!(result.stop_reason, Some(StopReason::User));
            }
        }
    }

    #[test]
    fn relative_swm_paths_use_the_selected_base_without_rewriting_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config.json");
        let content = br#"{ "swm": { "identity_path": "state/identity.json", "release_directory": "release" },
            "query": { "platform": "qualcomm", "scheme": "DIGEST", "device_code": "",
                       "pk_hash_sha256": "", "pk_hash_sha384": "", "msm_id": "01234567" } }"#;
        fs::write(&config, content).unwrap();
        for directory in ["first", "second"] {
            let base_directory = temp.path().join(directory);
            let input = CloudInput {
                config: config.clone(),
                output: "assets".into(),
                base_directory: Some(base_directory.clone()),
                identity_mode: IdentityMode::OpenExisting,
            };
            let resolved = resolve_config(&input).unwrap();
            let output = absolute_path(&input, &input.output).unwrap();
            let resolved = resolved.swm.unwrap();
            assert_eq!(
                resolved.identity_path,
                base_directory.join("state/identity.json")
            );
            assert_eq!(resolved.release_directory, base_directory.join("release"));
            assert_eq!(output, base_directory.join("assets"));
            assert!(!base_directory.exists());
        }
        assert_eq!(fs::read(config).unwrap(), content);
    }

    #[test]
    fn authenticated_clock_ignores_host_offsets_and_jumps_but_enforces_expiry() {
        let clock = Clock::Authenticated(crate::swm::AuthenticatedClock::for_test(
            1_000_000,
            std::time::Instant::now() - Duration::from_secs(2),
        ));
        let mut c = credentials();
        c.expires_at = 1600;
        let mut t = ticket(&c);
        validate_ticket(&t, &c, 11, 22, "loader", clock.now().unwrap()).unwrap();
        let before = clock.now().unwrap();
        for host in [990, 1000, 100_000, 1] {
            let time = clock.with_host_clock(|| Ok(host)).unwrap();
            assert!(time >= before && time < 1010);
            valid_credentials(&c, time).unwrap();
        }
        assert!(valid_credentials(&c, Clock::Host.with_host_clock(|| Ok(990)).unwrap()).is_err());
        assert_eq!(
            Clock::Host.with_host_clock(|| Ok(100_000)).unwrap(),
            100_000
        );
        c.expires_at = 1000;
        assert!(valid_credentials(&c, clock.now().unwrap()).is_err());
        t.expires_at_unix = 1000;
        assert!(
            validate_ticket(&t, &c, 11, 22, "loader", clock.now().unwrap())
                .unwrap_err()
                .to_string()
                .contains("expired or oversized")
        );
    }

    #[test]
    fn authenticated_clock_signs_http_requests_with_server_time() {
        use std::{
            io::{BufRead, BufReader},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let mut c = credentials();
        c.expires_at = 1600;
        let clock = Clock::Authenticated(crate::swm::AuthenticatedClock::for_test(
            1_000_000,
            std::time::Instant::now(),
        ));
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut headers = std::collections::BTreeMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let (k, v) = line.trim_end().split_once(':').unwrap();
                headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
            }
            let mut body = vec![0; headers["content-length"].parse().unwrap()];
            reader.read_exact(&mut body).unwrap();
            let time: u64 = headers["x-timestamp"].parse().unwrap();
            assert!((1000..1010).contains(&time));
            assert_eq!(
                headers["x-signature"],
                request_signature(
                    &credentials(),
                    "/api/v1/cloud-loader/list",
                    &body,
                    time,
                    &headers["x-nonce"]
                )
            );
            drop(reader);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .unwrap();
        });
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(post(
                &Client::new(),
                &origin,
                "/api/v1/cloud-loader/list",
                b"{}".to_vec(),
                Some(&c),
                &clock,
            ));
        assert_eq!(result.unwrap(), b"{}");
        server.join().unwrap();
    }
    fn credentials() -> Credentials {
        Credentials {
            client_id: "fixture-client".into(),
            client_secret: "fixture-secret".into(),
            delegation_id: "fixture-delegation".into(),
            expires_at: now().unwrap() + 600,
        }
    }
    fn ticket(c: &Credentials) -> Ticket {
        let mut t = Ticket {
            success: true,
            manifest_version: "cloud_loader_ticket_v2".into(),
            client_id: c.client_id.clone(),
            profile_id: 11,
            asset_id: 22,
            file_type: "loader".into(),
            size: 16,
            checksum_sha256: sha(b"fixture payload!"),
            expires_at_unix: now().unwrap() + 100,
            download_url: "https://example.invalid/object?ticket=fixture".into(),
            download_url_sha256: String::new(),
            manifest_signature: String::new(),
        };
        t.download_url_sha256 = sha(t.download_url.as_bytes());
        let manifest = format!(
            "cloud_loader_ticket_v2\nclient_id:{}\nprofile_id:11\nasset_id:22\nfile_type:loader\nsize:16\nsha256:{}\nexpires_at:{}\ndownload_url_sha256:{}",
            c.client_id, t.checksum_sha256, t.expires_at_unix, t.download_url_sha256
        );
        t.manifest_signature = signature(&c.client_secret, manifest.as_bytes());
        t
    }
    #[test]
    fn request_signature_matches_independent_python_hmac_vector() {
        assert_eq!(
            request_signature(
                &credentials(),
                "/api/v1/cloud-loader/list",
                br#"{"platform":"qualcomm"}"#,
                1700000000,
                "000102030405060708090a0b0c0d0e0f"
            ),
            "7d29f0366bf4fa3f49590e1b8aedc94afd80a0fc455a4d7707beb0e9acd58481"
        );
    }
    #[test]
    fn ticket_rejects_cross_binding_expiry_url_and_manifest_tampering() {
        let c = credentials();
        let t = ticket(&c);
        let time = now().unwrap();
        validate_ticket(&t, &c, 11, 22, "loader", time).unwrap();
        for change in ["size", "url", "signature", "expires", "profile", "type"] {
            let mut t = ticket(&c);
            match change {
                "size" => t.size += 1,
                "url" => t.download_url.push('x'),
                "signature" => t.manifest_signature = "00".repeat(32),
                "expires" => t.expires_at_unix = time,
                "profile" => t.profile_id = 12,
                _ => t.file_type = "sign".into(),
            }
            assert!(
                validate_ticket(&t, &c, 11, 22, "loader", time).is_err(),
                "{change}"
            );
        }
    }
    #[test]
    fn wreq_enroll_list_ticket_use_real_http_and_stop_before_unverified_asset() {
        let fixture = cloud_fixture::CloudFixture::start_with_validation(|path, headers, body| {
            if path == "enroll" {
                let request: serde_json::Value = serde_json::from_slice(body)?;
                assert_eq!(request["app_id"], crate::swm::APP);
            } else {
                let c = credentials();
                assert_eq!(headers["x-signature-version"], "1");
                assert_eq!(headers["x-nonce"].len(), 32);
                assert_eq!(
                    headers["x-signature"],
                    request_signature(
                        &c,
                        &format!("/api/v1/cloud-loader/{path}"),
                        body,
                        headers["x-timestamp"].parse()?,
                        &headers["x-nonce"]
                    )
                );
            }
            Ok(())
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("cloud.json");
        let out = dir.path().join("assets");
        cloud_fixture::write_config(&config).unwrap();
        let result = download_controlled(
            &CloudInput {
                config,
                output: out.clone(),
                base_directory: None,
                identity_mode: IdentityMode::OpenExisting,
            },
            &HostServices::default()
                .with_loopback(fixture.address)
                .unwrap(),
            None,
            &mut Report::new(None, None),
        );
        assert!(result.err().unwrap().to_string().contains("HMAC mismatch"));
        assert!(!out.exists());
        assert_eq!(fixture.finish().unwrap(), ["enroll", "list", "ticket"]);
    }

    fn query() -> Query {
        Query {
            platform: "qualcomm".into(),
            device_code: "23631".into(),
            scheme: "DIGEST".into(),
            pk_hash_sha256: String::new(),
            pk_hash_sha384: String::new(),
            msm_id: "002270E1".into(),
        }
    }
    #[test]
    #[ignore = "explicit live read-only cloud transport probe"]
    fn live_anonymous_list_reports_server_status() {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let client = Client::builder()
                    .timeout(Duration::from_secs(30))
                    .redirect(wreq::redirect::Policy::none())
                    .build()?;
                post(
                    &client,
                    ORIGIN,
                    "/api/v1/cloud-loader/list",
                    serde_json::to_vec(&query())?,
                    None,
                    &Clock::Host,
                )
                .await
            });
        let error = result.expect_err("anonymous list must not provide authenticated assets");
        eprintln!("Live wreq anonymous list: {error}");
        assert!(error.to_string().contains("401"));
    }
}
