//! Loopback legacy enrollment and ticket fixture. The ticket has an invalid HMAC.
#![allow(dead_code)]
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct CloudFixture {
    pub address: SocketAddr,
    stopped: Arc<AtomicBool>,
    server: Option<JoinHandle<Result<Vec<String>>>>,
}

impl CloudFixture {
    pub fn start() -> Result<Self> {
        Self::start_with_validation(|_, _, _| Ok(()))
    }

    pub fn start_with_validation(
        validate: impl Fn(&str, &BTreeMap<String, String>, &[u8]) -> Result<()> + Send + 'static,
    ) -> Result<Self> {
        Self::start_with_reply(validate, None)
    }

    pub fn start_with_malformed_ticket(marker: &str) -> Result<Self> {
        Self::start_with_reply(|_, _, _| Ok(()), Some(marker.to_owned()))
    }

    fn start_with_reply(
        validate: impl Fn(&str, &BTreeMap<String, String>, &[u8]) -> Result<()> + Send + 'static,
        malformed_ticket: Option<String>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let server = thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(5);
            let mut requests = Vec::new();
            for expected in ["enroll", "list", "ticket"] {
                let mut socket = loop {
                    ensure!(!stop.load(Ordering::Acquire), "fixture stopped");
                    ensure!(
                        Instant::now() < end,
                        "fixture deadline reached after {requests:?}"
                    );
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                socket.set_nonblocking(false)?;
                respond(
                    &mut socket,
                    end,
                    expected,
                    &validate,
                    malformed_ticket.as_deref(),
                )?;
                requests.push(expected.to_owned());
            }
            Ok(requests)
        });
        Ok(Self {
            address,
            stopped,
            server: Some(server),
        })
    }

    pub fn finish(mut self) -> Result<Vec<String>> {
        self.server
            .take()
            .unwrap()
            .join()
            .map_err(|_| anyhow::anyhow!("fixture server panicked"))?
    }
}

impl Drop for CloudFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

pub fn write_config(path: &Path) -> Result<()> {
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "enrollment_ticket": "fixture-swm-ticket",
            "query": {
                "platform": "qualcomm", "device_code": "23631", "scheme": "DIGEST",
                "pk_hash_sha256": "", "pk_hash_sha384": "", "msm_id": "002270E1"
            }
        }))?,
    )?;
    Ok(())
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let bound = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(1));
    if bound.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "fixture deadline reached",
        ));
    }
    Ok(bound)
}

struct FixtureReader<'a> {
    socket: &'a mut TcpStream,
    deadline: Instant,
}

impl Read for FixtureReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.socket
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        self.socket.read(out)
    }
}

fn respond(
    socket: &mut TcpStream,
    deadline: Instant,
    path: &str,
    validate: &impl Fn(&str, &BTreeMap<String, String>, &[u8]) -> Result<()>,
    malformed_ticket: Option<&str>,
) -> Result<()> {
    let mut reader = BufReader::new(FixtureReader { socket, deadline }.take(64 * 1024));
    let mut request = String::new();
    reader.read_line(&mut request)?;
    ensure!(
        request.starts_with(&format!("POST /api/v1/cloud-loader/{path} ")),
        "unexpected cloud request {request:?}"
    );
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        ensure!(reader.read_line(&mut line)? > 0, "truncated request");
        if line == "\r\n" {
            break;
        }
        let (key, value) = line
            .trim_end()
            .split_once(':')
            .context("malformed header")?;
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    let length: usize = headers
        .get("content-length")
        .context("missing content-length")?
        .parse()?;
    ensure!(length <= 32 * 1024, "request too large");
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    validate(path, &headers, &data)?;
    let body: Value = serde_json::from_slice(&data)?;
    if path == "enroll" {
        ensure!(
            body["ticket"] == "fixture-swm-ticket" && !headers.contains_key("x-signature"),
            "wrong enrollment body"
        );
    } else {
        ensure!(
            headers
                .get("x-client-id")
                .is_some_and(|id| id == "fixture-client")
                && headers.contains_key("x-signature"),
            "unsigned request"
        );
    }
    let expiry = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + 600;
    let url = "https://fixture.invalid/loader";
    let mut response = match path {
        "enroll" => {
            json!({"client_id": "fixture-client", "client_secret": "fixture-secret", "delegation_id": "fixture-delegation", "expires_at": expiry})
        }
        "list" => {
            ensure!(
                body["scheme"] == "DIGEST" && body["msm_id"] == "002270E1",
                "invalid configured query"
            );
            json!({"items": [{"id": 11, "platform": "qualcomm", "scheme": "DIGEST", "assets": [
                {"id": 22, "file_type": "loader"}, {"id": 23, "file_type": "digest"}, {"id": 24, "file_type": "sign"}
            ]}]})
        }
        _ => {
            ensure!(
                body["profile_id"] == 11 && body["asset_id"] == 22 && body["file_type"] == "loader",
                "wrong selected asset"
            );
            json!({
                "success": true, "manifest_version": "cloud_loader_ticket_v2",
                "client_id": "fixture-client", "profile_id": 11, "asset_id": 22,
                "file_type": "loader", "size": 4, "checksum_sha256": "02".repeat(32),
                "expires_at_unix": expiry, "download_url": url,
                "download_url_sha256": format!("{:x}", Sha256::digest(url.as_bytes())),
                "manifest_signature": "00".repeat(32)
            })
        }
    };
    if path == "ticket"
        && let Some(marker) = malformed_ticket
    {
        response["size"] = json!(marker);
    }
    let response = response.to_string();
    socket.set_write_timeout(Some(remaining(deadline)?))?;
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
        response.len()
    )?;
    Ok(())
}
