//! SWM preview protocol recovered from MySwm.dll 4.2026.009.
use anyhow::{Context, Result, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use wreq::Client;

pub const APP: &str = "abd975ae-a061-4137-a0c2-90ebdf17d3f2";
pub const RELEASE: &str = "df052167-1c7e-4e68-a32b-0dd485f1be41";
pub const VERSION: &str = "4.2026.009";
const ORIGIN: &str = "https://swm-backend.anteasy.com";
const ROOT_ID: &str = "app-abd975ae-20260815-root-101a";
const ROOT_KEY: &str = "7d48407568f38a54938c160ebd5853a8a4087857e53779aa8d23b6cfd24ccb5c";
const DOMAIN: &str = "swm-body-x25519-aes-gcm-v1";
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    ensure!(
        s.len() == N * 2 && s.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid SWM hex field"
    );
    let mut bytes = [0; N];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?;
    }
    Ok(bytes)
}
fn root_verify(message: &[u8], signature: &str) -> Result<()> {
    let key = VerifyingKey::from_bytes(&unhex::<32>(ROOT_KEY)?)?;
    let signature = Signature::from_slice(&STANDARD.decode(signature)?)?;
    key.verify_strict(message, &signature)
        .context("SWM root signature mismatch")
}
fn nonce() -> String {
    let mut n: [u8; 16] = rand::random();
    n[6] = n[6] & 15 | 64;
    n[8] = n[8] & 63 | 128;
    let h = hex(&n);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
#[derive(Deserialize)]
struct ServerTime {
    manifest_version: String,
    app_id: String,
    release_id: String,
    nonce: String,
    server_time_ms: u64,
    expires_at_ms: u64,
    root_trust_key_id: String,
    signature: String,
}
impl ServerTime {
    fn verify(&self, expected_nonce: &str) -> Result<()> {
        ensure!(
            self.manifest_version == "server_time_v1"
                && self.app_id == APP
                && self.release_id == RELEASE
                && self.nonce == expected_nonce
                && self.root_trust_key_id == ROOT_ID,
            "SWM signed time identity mismatch"
        );
        ensure!(
            self.server_time_ms > 0
                && self.expires_at_ms > self.server_time_ms
                && self.expires_at_ms - self.server_time_ms <= 60_000,
            "SWM signed time lifetime invalid"
        );
        root_verify(format!("server_time_v1\napp_id:{APP}\nrelease_id:{RELEASE}\nnonce:{}\nserver_time_ms:{}\nexpires_at_ms:{}\nroot_trust_key_id:{ROOT_ID}", self.nonce,self.server_time_ms,self.expires_at_ms).as_bytes(), &self.signature)
    }
}
#[derive(Deserialize)]
struct OnlineKey {
    manifest_version: String,
    purpose: String,
    app_id: String,
    release_id: String,
    key_id: String,
    public_key: String,
    root_trust_key_id: String,
    root_trust_signature: String,
    issued_at: u64,
    refresh_after: u64,
}
impl OnlineKey {
    fn verify(&self, now: u64) -> Result<()> {
        ensure!(
            self.manifest_version == "online_key_manifest_v1"
                && self.purpose == "online_body"
                && self.app_id == APP
                && self.release_id == RELEASE
                && self.root_trust_key_id == ROOT_ID
                && !self.key_id.is_empty()
                && self.key_id.len() <= 128,
            "SWM online key identity mismatch"
        );
        ensure!(
            self.issued_at <= now.saturating_add(120)
                && self.refresh_after >= now
                && self.refresh_after > self.issued_at
                && self.refresh_after - self.issued_at <= 2_592_000,
            "SWM online key lifetime invalid"
        );
        unhex::<32>(&self.public_key)?;
        root_verify(format!("online_key_manifest_v1\npurpose:online_body\napp_id:{APP}\nrelease_id:{RELEASE}\nkey_id:{}\npublic_key:{}\nroot_trust_key_id:{ROOT_ID}\nissued_at:{}\nrefresh_after:{}", self.key_id,self.public_key,self.issued_at,self.refresh_after).as_bytes(), &self.root_trust_signature)
    }
}
fn headers(request: wreq::RequestBuilder, nonce: &str) -> wreq::RequestBuilder {
    request
        .header("Accept", "application/json")
        .header("X-Nonce", nonce)
        .header("X-App-Id", APP)
        .header("X-Authz-Capability", "v3")
        .header("X-Client-Release-Id", RELEASE)
        .header("X-Client-Version", VERSION)
        .header("X-Client-Version-Code", "42026009")
}
async fn read(response: wreq::Response) -> Result<(u16, Vec<u8>)> {
    let status = response.status().as_u16();
    const LIMIT: usize = 4 * 1024 * 1024;
    ensure!(
        response.content_length().is_none_or(|n| n <= LIMIT as u64),
        "SWM response too large"
    );
    let mut stream = response.bytes_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read SWM response")?;
        ensure!(chunk.len() <= LIMIT - data.len(), "SWM response too large");
        data.extend_from_slice(&chunk);
    }
    Ok((status, data))
}
struct Session {
    client: Client,
    key: OnlineKey,
    clock: AuthenticatedClock,
}
/// Server time anchored to a monotonic instant, independent of the host wall clock.
pub(crate) struct AuthenticatedClock {
    time_ms: u64,
    received: Instant,
}
impl AuthenticatedClock {
    pub(crate) fn now(&self) -> u64 {
        ((u128::from(self.time_ms) + self.received.elapsed().as_millis()) / 1000)
            .min(u128::from(u64::MAX)) as u64
    }
    #[cfg(test)]
    pub(crate) fn for_test(time_ms: u64, received: Instant) -> Self {
        Self { time_ms, received }
    }
}
impl Session {
    async fn bootstrap() -> Result<Self> {
        let client = Client::builder()
            .redirect(wreq::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()?;
        let n = nonce();
        let started = Instant::now();
        let (status, data) = read(
            headers(client.get(format!("{ORIGIN}/api/client/time")), &n)
                .send()
                .await?,
        )
        .await?;
        ensure!(status == 200, "SWM signed time HTTP {status}");
        let time: ServerTime = serde_json::from_slice(&data).context("parse SWM signed time")?;
        time.verify(&n)?;
        ensure!(
            started.elapsed().as_millis() < (time.expires_at_ms - time.server_time_ms) as u128,
            "SWM signed time expired in transit"
        );
        let received = Instant::now();
        let (status, data) = read(
            headers(
                client.get(format!("{ORIGIN}/api/client/key-manifest")),
                &nonce(),
            )
            .header("X-Timestamp", (time.server_time_ms / 1000).to_string())
            .send()
            .await?,
        )
        .await?;
        ensure!(status == 200, "SWM online key HTTP {status}");
        let key: OnlineKey = serde_json::from_slice(&data).context("parse SWM online key")?;
        key.verify(time.server_time_ms / 1000)?;
        Ok(Self {
            client,
            key,
            clock: AuthenticatedClock {
                time_ms: time.server_time_ms,
                received,
            },
        })
    }
    fn now(&self) -> u64 {
        self.clock.now()
    }
}
// MySwm.dll 0x180058640: ephemeral key || GCM nonce || ciphertext || tag.
fn encrypt(
    key: &OnlineKey,
    path: &str,
    timestamp: u64,
    request_nonce: &str,
    plain: &[u8],
    secret: [u8; 32],
    iv: [u8; 12],
) -> Result<Vec<u8>> {
    use aes_gcm::{
        Aes256Gcm, KeyInit, Nonce,
        aead::{Aead, Payload},
    };
    use curve25519_dalek::{edwards::CompressedEdwardsY, montgomery::MontgomeryPoint};
    let point = CompressedEdwardsY(unhex::<32>(&key.public_key)?)
        .decompress()
        .context("invalid SWM Ed25519 point")?;
    ensure!(
        !point.is_small_order() && point.compress().to_bytes() == unhex::<32>(&key.public_key)?,
        "invalid SWM online key point"
    );
    let shared = point.to_montgomery().mul_clamped(secret).to_bytes();
    ensure!(shared != [0; 32], "SWM key agreement failed");
    let ephemeral = MontgomeryPoint::mul_base_clamped(secret).to_bytes();
    let context = format!(
        "{DOMAIN}\napp_id:{APP}\nrelease_id:{RELEASE}\nkey_id:{}\npublic_key:{}",
        key.key_id, key.public_key
    );
    let salt = Sha256::digest(context.as_bytes());
    let mut extract = <Hmac<Sha256> as Mac>::new_from_slice(&salt)?;
    extract.update(&shared);
    let mut expand = <Hmac<Sha256> as Mac>::new_from_slice(&extract.finalize().into_bytes())?;
    expand.update(
        format!(
            "{context}\nephemeral_public:{}",
            URL_SAFE_NO_PAD.encode(ephemeral)
        )
        .as_bytes(),
    );
    expand.update(&[1]);
    let aes_key = expand.finalize().into_bytes();
    let aad = format!(
        "{DOMAIN}\nPOST\n{path}\n\n\n{timestamp}\n{request_nonce}\n{APP}\nclient_release_id:{RELEASE}\nclient_version:{VERSION}\nclient_version_code:42026009\nauthz_capability:v3\nbody_enc:x25519-aes-gcm-v1\nkey_id:{}",
        key.key_id
    );
    let cipher =
        Aes256Gcm::new_from_slice(&aes_key).map_err(|_| anyhow::anyhow!("invalid SWM AES key"))?;
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: plain,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("SWM encryption failed"))?;
    let mut wire = ephemeral.to_vec();
    wire.extend_from_slice(&iv);
    wire.extend_from_slice(&encrypted);
    Ok(wire)
}

#[derive(Deserialize)]
pub struct Config {
    /// Private, persistent identity generated by this client, never an exported ticket.
    pub identity_path: std::path::PathBuf,
    /// Unmodified preview files used for signed release-manifest validation.
    pub release_directory: std::path::PathBuf,
}
#[derive(Deserialize, Serialize)]
struct Identity {
    install_id: String,
    secret: String,
}
impl Identity {
    fn load(path: &std::path::Path) -> Result<Self> {
        use std::io::{Read, Write};
        if path.try_exists()? {
            let mut data = Vec::new();
            crate::input::open_regular(path)?
                .take(4097)
                .read_to_end(&mut data)?;
            ensure!(data.len() <= 4096, "SWM identity too large");
            let identity: Self = serde_json::from_slice(&data).context("read SWM identity")?;
            identity.key()?;
            ensure!(
                identity.install_id.len() == 32
                    && identity.install_id.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid SWM installation ID"
            );
            return Ok(identity);
        }
        let key = loop {
            let secret: [u8; 32] = rand::random();
            if p256::ecdsa::SigningKey::from_slice(&secret).is_ok() {
                break secret;
            }
        };
        let identity = Self {
            install_id: hex(&rand::random::<[u8; 16]>()),
            secret: hex(&key),
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).context("create private SWM identity")?;
        file.write_all(&serde_json::to_vec(&identity)?)?;
        file.sync_all()?;
        Ok(identity)
    }
    fn key(&self) -> Result<p256::ecdsa::SigningKey> {
        Ok(p256::ecdsa::SigningKey::from_slice(&unhex::<32>(
            &self.secret,
        )?)?)
    }
    fn public(&self) -> Result<Vec<u8>> {
        Ok(self
            .key()?
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec())
    }
    fn thumbprint(&self) -> Result<String> {
        Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(self.public()?)))
    }
    fn key_id(&self) -> Result<String> {
        Ok(format!("swm-device-{}", &self.thumbprint()?[..22]))
    }
    fn pcid(&self) -> Result<String> {
        Ok(sha(format!(
            "device_credential_v2\napp_id:{APP}\ninstall_id:{}\nkey_thumbprint:{}",
            self.install_id,
            self.thumbprint()?
        )
        .as_bytes()))
    }
    fn proof(&self, path: &str, body: &[u8], session: &str, now: u64) -> Result<String> {
        use p256::ecdsa::{Signature, signature::Signer};
        let pcid = self.pcid()?;
        let header = serde_json::json!({"alg":"ES256","kid":self.key_id()?,"typ":"swm-dpop+jwt"});
        let payload = serde_json::json!({"app_id":APP,"ath":URL_SAFE_NO_PAD.encode(Sha256::digest(session.as_bytes())),
            "body_sha256":sha(body),"channel":"stable","device_id":pcid,"exp":now+60,"htm":"POST",
            "htu":format!("{ORIGIN}{path}"),"iat":now,"install_id":self.install_id,
            "jti":hex(&rand::random::<[u8;16]>()),"pcid":pcid,"release_id":RELEASE});
        let signing = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?)
        );
        let signature: Signature = self.key()?.sign(signing.as_bytes());
        Ok(format!(
            "{signing}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }
}
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .context("SWM manifest offset overflow")?;
        let b = self
            .bytes
            .get(self.pos..end)
            .context("truncated SWM manifest")?;
        self.pos = end;
        Ok(b)
    }
    fn number(&mut self, n: usize) -> Result<u64> {
        Ok(self.take(n)?.iter().fold(0, |v, b| v << 8 | *b as u64))
    }
    fn string(&mut self, n: usize) -> Result<String> {
        let len = self.number(n)? as usize;
        Ok(std::str::from_utf8(self.take(len)?)?.to_owned())
    }
}
fn verify_release(root: &std::path::Path) -> Result<serde_json::Value> {
    use std::io::Read;
    let mut data = Vec::new();
    crate::input::open_regular(&root.join("Oplusdll/release-integrity.v2"))?
        .take(262145)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 262144, "release manifest too large");
    let mut wire = Cursor {
        bytes: &data,
        pos: 0,
    };
    ensure!(
        wire.take(8)? == b"OPLUSRIM" && wire.number(2)? == 2,
        "invalid release manifest header"
    );
    let len = wire.number(4)? as usize;
    let payload = wire.take(len)?;
    let root_id = wire.string(1)?;
    let signing_id = wire.string(1)?;
    let key: [u8; 32] = wire.take(32)?.try_into()?;
    let certificate = Signature::from_slice(wire.take(64)?)?;
    let signature = Signature::from_slice(wire.take(64)?)?;
    ensure!(
        wire.pos == data.len() && root_id == ROOT_ID,
        "invalid release manifest certificate"
    );
    let mut signed = b"OPLUS_RELEASE_KEY_V2\0".to_vec();
    signed.extend_from_slice(payload.get(..16).context("short manifest identity")?);
    for s in [&root_id, &signing_id] {
        ensure!(s.len() <= 64, "invalid release key ID");
        signed.push(s.len() as u8);
        signed.extend_from_slice(s.as_bytes());
    }
    signed.extend_from_slice(&key);
    VerifyingKey::from_bytes(&unhex::<32>(ROOT_KEY)?)?
        .verify_strict(&signed, &certificate)
        .context("release key certificate mismatch")?;
    let mut signed = b"OPLUS_RELEASE_MANIFEST_V2\0".to_vec();
    signed.extend_from_slice(payload);
    VerifyingKey::from_bytes(&key)?
        .verify_strict(&signed, &signature)
        .context("release manifest signature mismatch")?;
    let mut p = Cursor {
        bytes: payload,
        pos: 0,
    };
    ensure!(
        hex(p.take(16)?) == APP.replace('-', "") && hex(p.take(16)?) == RELEASE.replace('-', ""),
        "release manifest identity mismatch"
    );
    ensure!(
        p.number(8)? == 42026009
            && p.number(1)? == 1
            && p.number(1)? == 2
            && p.string(2)? == VERSION,
        "release manifest version mismatch"
    );
    let count = p.number(2)?;
    ensure!(count > 0 && count <= 256, "invalid release file count");
    let root = root.canonicalize()?;
    let mut files = serde_json::Map::new();
    for _ in 0..count {
        let name = p.string(2)?;
        let size = p.number(8)?;
        let expected = p.take(32)?;
        ensure!(
            !name.is_empty()
                && !name.contains('\\')
                && std::path::Path::new(&name)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))),
            "unsafe release file path"
        );
        let path = root.join(&name).canonicalize()?;
        ensure!(path.starts_with(&root), "release file escapes directory");
        let mut file = std::fs::File::open(path)?;
        ensure!(
            file.metadata()?.len() == size,
            "release file size mismatch: {name}"
        );
        let mut hash = Sha256::new();
        let mut buf = [0; 65536];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        let actual = hash.finalize();
        ensure!(
            &actual[..] == expected,
            "release file hash mismatch: {name}"
        );
        ensure!(
            files
                .insert(name, serde_json::Value::String(hex(&actual)))
                .is_none(),
            "duplicate release file"
        );
    }
    ensure!(p.pos == payload.len(), "release manifest trailing payload");
    Ok(
        serde_json::json!({"integrity_evidence_version":2,"integrity_state":"verified","integrity_manifest_sha256":sha(&data),"integrity_files":files}),
    )
}
impl Session {
    async fn post(
        &self,
        identity: &Identity,
        path: &str,
        value: &serde_json::Value,
        session: &str,
    ) -> Result<(u16, Vec<u8>, String)> {
        let now = self.now();
        self.key.verify(now)?;
        let body = serde_json::to_vec(value)?;
        let n = nonce();
        let proof = identity.proof(path, &body, session, now)?;
        let encrypted = encrypt(
            &self.key,
            path,
            now,
            &n,
            &body,
            rand::random(),
            rand::random(),
        )?;
        let mut request = headers(self.client.post(format!("{ORIGIN}{path}")), &n)
            .header("X-Timestamp", now.to_string())
            .header("Content-Type", "application/json; charset=utf-8")
            .header("X-SWM-DPoP", proof)
            .header("X-SWM-Body-Enc", "x25519-aes-gcm-v1")
            .header("X-SWM-Online-Key-Id", &self.key.key_id);
        if !session.is_empty() {
            request = request.header("X-SWM-Session", session);
        }
        let (status, data) = read(
            request
                .body(encrypted)
                .send()
                .await
                .context("send SWM request")?,
        )
        .await?;
        Ok((status, data, n))
    }
}
fn registration_body(
    identity: &Identity,
    evidence: serde_json::Value,
) -> Result<serde_json::Value> {
    let mut body = serde_json::json!({"channel_code":"stable","current_version":VERSION,"version_code":42026009,
        "platform":"windows","arch":"x64","device_id":identity.pcid()?,"attributes":{},
        "device_auth":{"install_id":identity.install_id,"key_id":identity.key_id()?,"key_thumbprint":identity.thumbprint()?,
            "public_key_sec1":URL_SAFE_NO_PAD.encode(identity.public()?),"credential_version":"device_credential_v2",
            "hardware_evidence":hardware_evidence()?}});
    body.as_object_mut().context("registration object")?.extend(
        evidence
            .as_object()
            .context("release evidence object")?
            .clone(),
    );
    Ok(body)
}
async fn register(config: &Config) -> Result<(Session, Identity, (u16, Vec<u8>, String))> {
    let evidence = verify_release(&config.release_directory)?;
    let identity = Identity::load(&config.identity_path)?;
    let swm = Session::bootstrap().await?;
    let mut body = registration_body(&identity, evidence)?;
    let mut response = swm
        .post(&identity, "/api/client/update-check", &body, "")
        .await?;
    if response.0 == 428 {
        #[derive(Deserialize)]
        struct Challenge {
            challenge: String,
            expires_at: u64,
        }
        let challenge: Challenge =
            serde_json::from_slice(&response.1).context("parse SWM registration challenge")?;
        ensure!(
            !challenge.challenge.is_empty()
                && challenge.challenge.len() <= 512
                && challenge.expires_at > swm.now(),
            "invalid or expired SWM registration challenge"
        );
        body["device_auth"]["challenge"] = challenge.challenge.into();
        response = swm
            .post(&identity, "/api/client/update-check", &body, "")
            .await?;
    }
    Ok((swm, identity, response))
}
// The machine_guid component uses the platform's stable machine identifier.
// Raw identifiers stay local; only the aggregate digest is sent.
fn hardware_evidence() -> Result<serde_json::Value> {
    #[cfg(target_os = "macos")]
    let value = {
        let output = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()?;
        ensure!(
            output.status.success(),
            "read macOS platform identity failed"
        );
        let text = String::from_utf8(output.stdout)?;
        text.lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                key.contains("\"IOPlatformUUID\"")
                    .then(|| value.trim().trim_matches('"').to_owned())
            })
            .context("macOS platform identity unavailable")?
    };
    #[cfg(target_os = "linux")]
    let value =
        std::fs::read_to_string("/etc/machine-id").context("read Linux machine identity")?;
    #[cfg(target_os = "windows")]
    let value = {
        let output=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","(Get-ItemProperty -LiteralPath 'HKLM:\\SOFTWARE\\Microsoft\\Cryptography').MachineGuid"]).output()?;
        ensure!(
            output.status.success(),
            "read Windows machine identity failed"
        );
        String::from_utf8(output.stdout)?
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    anyhow::bail!("SWM machine identity collection unsupported on this platform");
    let normalized: String = value
        .chars()
        .filter(|c| *c > ' ' && *c != '\u{7f}')
        .flat_map(char::to_lowercase)
        .collect();
    ensure!(
        !normalized.is_empty() && normalized.len() <= 256,
        "invalid machine identity"
    );
    let aggregate = sha(format!(
        "device_hardware_evidence_v2\napp_id:{APP}\nmachine_guid:{normalized}\n"
    )
    .as_bytes());
    Ok(serde_json::json!({"version":2,"component_mask":32,"aggregate_hash":aggregate}))
}

#[derive(Deserialize)]
struct Carrier {
    authz: Authorization,
    data: Box<serde_json::value::RawValue>,
}
#[derive(Deserialize)]
struct Authorization {
    version: String,
    decision: String,
    release_id: String,
    device_id: String,
    nonce: String,
    #[serde(default)]
    reason: String,
    data_sha256: String,
    session: String,
    issued_at: u64,
    expires_at: u64,
    key_id: String,
    signature: String,
}
impl Session {
    fn verify_response(
        &self,
        identity: &Identity,
        response: (u16, Vec<u8>, String),
    ) -> Result<Carrier> {
        let (status, body, nonce) = response;
        if !(200..300).contains(&status) {
            let error: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let code = error["error"]["code"].as_str().unwrap_or("unknown");
            let safe: String = code
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(128)
                .collect();
            anyhow::bail!("SWM HTTP {status}: {safe}");
        }
        let carrier: Carrier =
            serde_json::from_slice(&body).context("parse SWM signed response")?;
        let a = &carrier.authz;
        let now = self.now();
        ensure!(
            a.version == "authz_v3"
                && a.decision == "allow"
                && a.release_id == RELEASE
                && a.device_id == identity.pcid()?
                && a.nonce == nonce
                && a.key_id == self.key.key_id,
            "SWM authorization identity mismatch or access denied"
        );
        ensure!(
            !a.session.is_empty()
                && a.session.len() <= 512
                && a.reason.len() <= 128
                && a.issued_at <= now.saturating_add(120)
                && a.expires_at > now
                && a.expires_at > a.issued_at
                && a.expires_at - a.issued_at <= 900,
            "SWM authorization lifetime invalid"
        );
        ensure!(
            sha(carrier.data.get().as_bytes()) == a.data_sha256,
            "SWM response data hash mismatch"
        );
        let signed = format!(
            "authz_v3\napp_id:{APP}\nrelease_id:{RELEASE}\ndevice_id:{}\nnonce:{}\ndecision:{}\nreason:{}\ndata_sha256:{}\nsession:{}\nissued_at:{}\nexpires_at:{}\nkey_id:{}",
            a.device_id,
            a.nonce,
            a.decision,
            a.reason,
            a.data_sha256,
            a.session,
            a.issued_at,
            a.expires_at,
            a.key_id
        );
        let signature = Signature::from_slice(&STANDARD.decode(&a.signature)?)?;
        VerifyingKey::from_bytes(&unhex::<32>(&self.key.public_key)?)?
            .verify_strict(signed.as_bytes(), &signature)
            .context("SWM authorization signature mismatch")?;
        Ok(carrier)
    }
}
/// Obtain a fresh SWM ticket through signed registration and enrollment.
/// Credentials and session tokens are never included in diagnostics or saved to disk.
pub async fn enrollment_ticket(config: &Config) -> Result<String> {
    Ok(enrollment_with_clock(config).await?.0)
}
pub(crate) async fn enrollment_with_clock(config: &Config) -> Result<(String, AuthenticatedClock)> {
    let (swm, identity, response) = register(config).await?;
    let registered = swm.verify_response(&identity, response)?;
    let response = swm
        .post(
            &identity,
            "/api/client/enrollment-ticket",
            &serde_json::json!({"audience":"cloud_loader"}),
            &registered.authz.session,
        )
        .await?;
    let enrolled = swm.verify_response(&identity, response)?;
    #[derive(Deserialize)]
    struct Ticket {
        ticket: String,
        audience: String,
        expires_at: u64,
    }
    let ticket: Ticket =
        serde_json::from_str(enrolled.data.get()).context("parse SWM enrollment ticket")?;
    ensure!(
        ticket.audience == "cloud_loader"
            && !ticket.ticket.is_empty()
            && ticket.ticket.len() <= 4096
            && ticket.expires_at > swm.now(),
        "invalid SWM enrollment ticket"
    );
    Ok((ticket.ticket, swm.clock))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn identity_special_input_child() {
        let Some(path) = std::env::var_os("OPLUS_EDL_SWM_IDENTITY_TEST") else {
            return;
        };
        let error = Identity::load(std::path::Path::new(&path)).err().unwrap();
        assert!(format!("{error:#}").contains("not a regular file"));
    }
    #[cfg(unix)]
    #[test]
    fn identity_rejects_fifo_and_symlink_without_waiting_for_writer() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::symlink},
            process::{Command, Stdio},
        };
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("identity-fifo");
        let c_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: c_path is a valid NUL-terminated path and mkfifo retains no pointer.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let link = dir.path().join("identity-link");
        symlink(&fifo, &link).unwrap();
        for path in [&fifo, &link] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "swm::tests::identity_special_input_child",
                    "--nocapture",
                ])
                .env("OPLUS_EDL_SWM_IDENTITY_TEST", path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!("identity loading blocked on FIFO without a writer");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    #[test]
    fn captured_root_signatures_and_nonce_binding() {
        let mut time: ServerTime =
            serde_json::from_str(include_str!("../tests/fixtures/swm/time.json")).unwrap();
        let nonce = include_str!("../tests/fixtures/swm/time-nonce.txt");
        time.verify(nonce).unwrap();
        assert!(time.verify("replayed nonce").is_err());
        time.server_time_ms += 1;
        assert!(time.verify(nonce).is_err());
        let mut key: OnlineKey =
            serde_json::from_str(include_str!("../tests/fixtures/swm/key-manifest.json")).unwrap();
        key.verify(key.issued_at).unwrap();
        assert!(key.verify(key.refresh_after + 1).is_err());
        key.public_key.replace_range(..2, "00");
        assert!(key.verify(key.issued_at).is_err());
    }
    #[test]
    fn identity_survives_restart_and_dpop_binds_body() {
        use p256::ecdsa::{Signature, signature::Verifier};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.json");
        let a = Identity::load(&path).unwrap();
        let b = Identity::load(&path).unwrap();
        assert_eq!(a.pcid().unwrap(), b.pcid().unwrap());
        assert_eq!(a.install_id.len(), 32);
        let proof = b.proof("/api/client/update-check", b"{}", "", 123).unwrap();
        let parts: Vec<_> = proof.split('.').collect();
        let payload: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(payload["body_sha256"], sha(b"{}"));
        assert_eq!(payload["device_id"], a.pcid().unwrap());
        let signed = format!("{}.{}", parts[0], parts[1]);
        let sig = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        a.key()
            .unwrap()
            .verifying_key()
            .verify(signed.as_bytes(), &sig)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn signed_response_rejects_replay_and_changed_raw_data() {
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut online: OnlineKey =
            serde_json::from_str(include_str!("../tests/fixtures/swm/key-manifest.json")).unwrap();
        online.public_key = hex(key.verifying_key().as_bytes());
        online.key_id = "fixture-online".into();
        let session = Session {
            client: Client::new(),
            key: online,
            clock: AuthenticatedClock::for_test(1_000_000, Instant::now()),
        };
        let identity = Identity {
            install_id: "a".repeat(32),
            secret: hex(&[1; 32]),
        };
        let pcid = identity.pcid().unwrap();
        let hash = sha(b"{ \"ok\": true }");
        let signed = format!(
            "authz_v3\napp_id:{APP}\nrelease_id:{RELEASE}\ndevice_id:{pcid}\nnonce:test-nonce\ndecision:allow\nreason:\ndata_sha256:{hash}\nsession:fixture-session\nissued_at:999\nexpires_at:1100\nkey_id:fixture-online"
        );
        let signature = STANDARD.encode(key.sign(signed.as_bytes()).to_bytes());
        let authz = serde_json::json!({"version":"authz_v3","decision":"allow","release_id":RELEASE,"device_id":pcid,
            "nonce":"test-nonce","data_sha256":hash,"session":"fixture-session","issued_at":999,"expires_at":1100,
            "key_id":"fixture-online","signature":signature});
        let wire = format!("{{\"authz\":{authz},\"data\":{{ \"ok\": true }}}}").into_bytes();
        session
            .verify_response(&identity, (200, wire.clone(), "test-nonce".into()))
            .unwrap();
        assert!(
            session
                .verify_response(&identity, (200, wire.clone(), "other-nonce".into()))
                .is_err()
        );
        let changed = String::from_utf8(wire)
            .unwrap()
            .replace("{ \"ok\": true }", "{\"ok\":true}");
        assert!(
            session
                .verify_response(&identity, (200, changed.into_bytes(), "test-nonce".into()))
                .is_err()
        );
        let json = r#"{"authz":{"version":"authz_v3","version":"authz_v3"},"data":{}}"#;
        assert!(serde_json::from_str::<Carrier>(json).is_err());
    }
}
