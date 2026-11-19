// SPDX-License-Identifier: BSD-3-Clause
#![allow(dead_code)] // Shared by independent integration-test binaries.

use std::{
    collections::VecDeque,
    io::{self, BufRead, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
};

use qdl::types::{FirehoseConfiguration, QdlBackend, QdlDevice, QdlReadWrite};

pub const ACK: &[u8] = b"<data><response value=\"ACK\"/></data>";
pub const NAK: &[u8] = b"<data><response value=\"NAK\"/></data>";
pub const LOG: &[u8] = b"<data><log value=\"hello\"/></data>";

pub enum Input {
    Bytes(Vec<u8>),
    Eof,
    Error(ErrorKind),
    Expected {
        access: Access,
        input: Box<Input>,
        refill: Option<Hook>,
        consume: Option<Hook>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Ordinary,
    Response,
}

type Hook = Mutex<Box<dyn FnOnce() + Send>>;

pub fn expect_response(input: Input) -> Input {
    expected(Access::Response, input, None, None)
}

pub fn on_refill(access: Access, input: Input, hook: impl FnOnce() + Send + 'static) -> Input {
    expected(access, input, Some(Mutex::new(Box::new(hook))), None)
}

pub fn on_consume(access: Access, input: Input, hook: impl FnOnce() + Send + 'static) -> Input {
    expected(access, input, None, Some(Mutex::new(Box::new(hook))))
}

fn expected(access: Access, input: Input, refill: Option<Hook>, consume: Option<Hook>) -> Input {
    Input::Expected {
        access,
        input: Box::new(input),
        refill,
        consume,
    }
}

pub fn bytes(value: &[u8]) -> Input {
    Input::Bytes(value.to_vec())
}

pub enum Output {
    Accept(usize),
    Error(ErrorKind),
}

#[derive(Default, Debug)]
pub struct Record {
    pub accepted: Vec<u8>,
    pub writes: Vec<Vec<u8>>,
    pub flushes: usize,
    pub refills: Vec<Access>,
    pub consumed: usize,
    pub closed: bool,
}

pub type Trace = Arc<Mutex<Record>>;

pub struct ScriptedIo {
    input: VecDeque<Input>,
    pending: Vec<u8>,
    pos: usize,
    expected: Option<Access>,
    on_consume: Option<Hook>,
    pub output: VecDeque<Output>,
    pub flush_error: Option<ErrorKind>,
    pub on_drop: Option<Hook>,
    pub trace: Trace,
}

impl ScriptedIo {
    pub fn new(input: impl IntoIterator<Item = Input>) -> Self {
        Self {
            input: input.into_iter().collect(),
            pending: Vec::new(),
            pos: 0,
            expected: None,
            on_consume: None,
            output: VecDeque::new(),
            flush_error: None,
            on_drop: None,
            trace: Trace::default(),
        }
    }
    fn refill(&mut self, access: Access) -> io::Result<&[u8]> {
        self.trace.lock().unwrap().refills.push(access);
        if self.pos == self.pending.len() {
            self.pending.clear();
            self.pos = 0;
            self.expected = None;
            let input = match self.input.pop_front().expect("unscripted read") {
                Input::Expected {
                    access: goal,
                    input,
                    refill,
                    consume,
                } => {
                    assert_eq!(access, goal, "wrong transport entry point");
                    self.expected = Some(goal);
                    self.on_consume = consume;
                    if let Some(hook) = refill {
                        hook.into_inner().unwrap()();
                    }
                    *input
                }
                input => input,
            };
            match input {
                Input::Bytes(data) => {
                    assert!(!data.is_empty(), "use explicit EOF for zero progress");
                    self.pending = data;
                }
                Input::Eof => return Ok(&[]),
                Input::Error(kind) => return Err(io::Error::new(kind, "scripted read failure")),
                Input::Expected { .. } => panic!("nested refill expectation"),
            }
        }
        if let Some(expected) = self.expected {
            assert_eq!(access, expected, "wrong entry point for buffered data");
        }
        Ok(&self.pending[self.pos..])
    }
}

impl BufRead for ScriptedIo {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.refill(Access::Ordinary)
    }

    fn consume(&mut self, amt: usize) {
        assert!(amt <= self.pending.len() - self.pos);
        self.pos += amt;
        self.trace.lock().unwrap().consumed += amt;
        if let Some(hook) = self.on_consume.take() {
            hook.into_inner().unwrap()();
        }
    }
}

impl Read for ScriptedIo {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let available = self.fill_buf()?;
        let len = out.len().min(available.len());
        out[..len].copy_from_slice(&available[..len]);
        self.consume(len);
        Ok(len)
    }
}

impl Write for ScriptedIo {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut record = self.trace.lock().unwrap();
        record.writes.push(data.to_vec());
        let len = match self.output.pop_front() {
            Some(Output::Accept(limit)) => data.len().min(limit),
            Some(Output::Error(kind)) => {
                return Err(io::Error::new(kind, "scripted write failure"));
            }
            None => data.len(),
        };
        record.accepted.extend_from_slice(&data[..len]);
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.trace.lock().unwrap().flushes += 1;
        match self.flush_error.take() {
            Some(kind) => Err(io::Error::new(kind, "scripted flush failure")),
            None => Ok(()),
        }
    }
}

impl QdlReadWrite for ScriptedIo {
    fn fill_response_buf(&mut self) -> io::Result<&[u8]> {
        self.refill(Access::Response)
    }
}

impl Drop for ScriptedIo {
    fn drop(&mut self) {
        if let Some(gate) = self.on_drop.take() {
            gate.into_inner().unwrap()();
        }
        self.trace.lock().unwrap().closed = true;
    }
}

pub fn device(input: impl IntoIterator<Item = Input>) -> (QdlDevice<ScriptedIo>, Trace) {
    let io = ScriptedIo::new(input);
    let trace = io.trace.clone();
    (
        QdlDevice {
            rw: Box::new(io),
            fh_cfg: FirehoseConfiguration {
                storage_sector_size: 4,
                send_buffer_size: 8,
                recv_buffer_size: 8,
                backend: QdlBackend::Serial,
                quiet: true,
                ..Default::default()
            },
            reset_on_drop: false,
        },
        trace,
    )
}

pub fn assert_no_reset(trace: &Trace) {
    let record = trace.lock().unwrap();
    let data = String::from_utf8_lossy(&record.accepted);
    for forbidden in ["<power", "<patch", "<setbootablestoragedrive"] {
        assert!(!data.contains(forbidden), "unexpected command: {data}");
    }
}

pub fn disk(sector: usize, names: &[&str]) -> Vec<u8> {
    disk_with_sectors(sector, 256, names)
}

fn disk_with_sectors(sector: usize, sectors: usize, names: &[&str]) -> Vec<u8> {
    let mut disk = std::io::Cursor::new(vec![0; sectors * sector]);
    let mut gpt = gptman::GPT::new_from(&mut disk, sector as u64, [42; 16]).unwrap();
    for (index, name) in names.iter().enumerate() {
        gpt[(index + 1) as u32] = gptman::GPTPartitionEntry {
            partition_type_guid: [1; 16],
            unique_partition_guid: [index as u8 + 1; 16],
            starting_lba: 64 + index as u64 * 4,
            ending_lba: 65 + index as u64 * 4,
            attribute_bits: 0,
            partition_name: (*name).into(),
        };
    }
    gpt.write_into(&mut disk).unwrap();
    disk.into_inner()
}
pub fn gpt_script(disk: &[u8], sector: usize) -> Vec<Input> {
    let h = gptman::GPTHeader::read_from(&mut std::io::Cursor::new(&disk[sector..])).unwrap();
    vec![
        bytes(ACK),
        bytes(&disk[sector..2 * sector]),
        bytes(ACK),
        bytes(ACK),
        bytes(&disk[..h.first_usable_lba as usize * sector]),
        bytes(ACK),
    ]
}
pub fn storage_device(
    input: impl IntoIterator<Item = Input>,
    sector: usize,
) -> (QdlDevice<ScriptedIo>, Trace) {
    let (mut dev, trace) = device(input);
    dev.fh_cfg.storage_sector_size = sector;
    dev.fh_cfg.send_buffer_size = 4096;
    dev.fh_cfg.recv_buffer_size = 4096;
    (dev, trace)
}

pub const RAW_ACK: &[u8] = b"<data><response value=\"ACK\" rawmode=\"true\"/></data>";
pub const WELCOME_END: &[u8] = b"<data><log value=\"INFO: End of supported functions\"/></data>";
pub const CONFIG_ACK: &[u8] = b"<data><response value=\"ACK\" MinVersionSupported=\"1\" Version=\"1\" MaxPayloadSizeToTargetInBytesSupported=\"4096\" MaxPayloadSizeToTargetInBytes=\"4096\" MaxXMLSizeInBytes=\"4096\"/></data>";
pub const STORAGE_ACK: &[u8] =
    b"<data><response value=\"ACK\" num_physical_partitions=\"1\"/></data>";

pub fn response(packet: &[u8]) -> Input {
    expect_response(bytes(packet))
}

pub fn sahara_script() -> Vec<Input> {
    fn packet(command: u32, words: &[u32]) -> Input {
        Input::Bytes(
            [command, 8 + 4 * words.len() as u32]
                .into_iter()
                .chain(words.iter().copied())
                .flat_map(u32::to_le_bytes)
                .collect(),
        )
    }
    vec![
        packet(1, &[2, 1, 4096, 0, 0, 0, 0, 0, 0, 0]),
        packet(3, &[13, 0, 4]),
        packet(4, &[13, 0]),
        packet(6, &[1]),
    ]
}

pub fn initialization(digest: bool) -> Vec<Input> {
    let mut input = sahara_script();
    input.push(response(WELCOME_END));
    if digest {
        input.extend([
            response(ACK),
            response(RAW_ACK),
            response(ACK),
            response(ACK),
        ]);
    }
    input.push(response(CONFIG_ACK));
    input
}

/// Read-only GPT and OCDT exchanges for an SFP package on a scripted UFS device.
pub fn package_project_script(project: u32) -> Vec<Input> {
    let mut script = initialization(false);
    let ordinary = disk_with_sectors(4096, 1024, &[]);
    let protected = disk_with_sectors(4096, 1024, &["ocdt"]);
    for lun in 0..6 {
        script.extend(gpt_script(
            if lun == 5 { &protected } else { &ordinary },
            4096,
        ));
    }
    let mut block = [0u8; 4096];
    block[..16].copy_from_slice(b"TDCO\x06\0\0\0\x10\0\0\0\x34\0\0\0");
    let content = project.to_le_bytes();
    for (index, out) in block[16..68].iter_mut().enumerate() {
        let byte = if index < 12 { content[index % 4] } else { 0 };
        *out = (byte ^ b"geyixue"[index % 7]).rotate_left(4);
    }
    script.extend([response(RAW_ACK), bytes(&block), response(ACK)]);
    script
}

pub fn scripted_host(input: Vec<Input>) -> (oplus_edl_core::HostServices, Trace) {
    let transport = ScriptedIo::new(input);
    let trace = transport.trace.clone();
    let once = Mutex::new(Some(transport));
    let host = oplus_edl_core::HostServices::default().with_backend(
        oplus_edl_core::model::Backend::Usb,
        || Ok(vec!["scripted".into()]),
        move |key, cfg| {
            assert_eq!(key, "scripted");
            Ok(QdlDevice {
                rw: Box::new(once.lock().unwrap().take().expect("unexpected reconnect")),
                fh_cfg: cfg,
                reset_on_drop: false,
            })
        },
    );
    (host, trace)
}

#[derive(Clone, Default)]
pub struct ManualClock(Arc<std::sync::atomic::AtomicU64>);

impl ManualClock {
    pub fn clock(&self) -> oplus_edl_core::service::Clock {
        let offset = self.0.clone();
        let origin = std::time::Instant::now();
        Arc::new(move || {
            origin
                + std::time::Duration::from_secs(offset.load(std::sync::atomic::Ordering::SeqCst))
        })
    }

    pub fn set(&self, seconds: u64) {
        self.0.store(seconds, std::sync::atomic::Ordering::SeqCst);
    }
}

const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Pause {
    arrived: std::sync::mpsc::Receiver<()>,
    release: Option<std::sync::mpsc::Sender<()>>,
}

impl Pause {
    pub fn new() -> (Self, impl FnOnce() + Send + 'static) {
        let (reach, arrived) = std::sync::mpsc::channel();
        let (release, proceed) = std::sync::mpsc::channel();
        (
            Self {
                arrived,
                release: Some(release),
            },
            move || {
                if reach.send(()).is_ok() {
                    let _ = proceed.recv_timeout(WAIT);
                }
            },
        )
    }

    pub fn arrive(&self) {
        self.arrived
            .recv_timeout(WAIT)
            .expect("worker did not reach the scripted boundary");
    }

    pub fn release(mut self) {
        let _ = self.release.take().unwrap().send(());
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}

pub fn pause_response(input: Input) -> (Input, Pause) {
    let (pause, callback) = Pause::new();
    (on_refill(Access::Response, input, callback), pause)
}

pub fn wait_for<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let start = std::time::Instant::now();
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(start.elapsed() < WAIT, "worker did not publish its result");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

pub fn wire_commands(trace: &Trace) -> Vec<xmltree::Element> {
    trace
        .lock()
        .unwrap()
        .writes
        .iter()
        .filter_map(|bytes| xmltree::Element::parse(bytes.as_slice()).ok())
        .flat_map(|packet| {
            packet.children.into_iter().filter_map(|node| {
                if let xmltree::XMLNode::Element(command) = node {
                    Some(command)
                } else {
                    None
                }
            })
        })
        .collect()
}

pub struct DeadlineChannel {
    inner: QdlDevice<ScriptedIo>,
    clock: oplus_edl_core::service::Clock,
    stopped: std::time::Instant,
}

pub fn deadline_channel(input: Vec<Input>, clock: &ManualClock) -> (DeadlineChannel, Trace) {
    let (inner, trace) = storage_device(input, 512);
    let clock = clock.clock();
    let stopped = clock();
    (
        DeadlineChannel {
            inner,
            clock,
            stopped,
        },
        trace,
    )
}

impl Read for DeadlineChannel {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.inner.read(bytes)
    }
}
impl BufRead for DeadlineChannel {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, n: usize) {
        self.inner.consume(n);
    }
}
impl Write for DeadlineChannel {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl qdl::types::QdlChan for DeadlineChannel {
    fn fh_config(&self) -> &FirehoseConfiguration {
        self.inner.fh_config()
    }
    fn mut_fh_config(&mut self) -> &mut FirehoseConfiguration {
        self.inner.mut_fh_config()
    }
    fn fill_response_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_response_buf()
    }
    fn check_response_wait(&self) -> anyhow::Result<()> {
        if (self.clock)().saturating_duration_since(self.stopped)
            >= std::time::Duration::from_secs(10)
        {
            return Err(oplus_edl_core::session::ResponseWaitExpired.into());
        }
        Ok(())
    }
}
