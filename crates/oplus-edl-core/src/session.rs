use crate::{
    host::DeviceChannel,
    model::{SessionId, StopReason},
    result::Target,
    service::Job,
};
use qdl::types::{FirehoseConfiguration, MutationFailure, QdlChan};
use std::{
    fmt,
    io::{self, BufRead, Read, Write},
    time::{Duration, Instant},
};

pub(crate) const RESPONSE_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseWaitExpired;

impl fmt::Display for ResponseWaitExpired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Firehose response wait exceeded the ten-second stop grace period")
    }
}
impl std::error::Error for ResponseWaitExpired {}

#[derive(Debug)]
pub(crate) struct Stopped;

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stopped before the next device transaction")
    }
}
impl std::error::Error for Stopped {}

pub(crate) fn response_wait_expired(error: &anyhow::Error) -> bool {
    find_source::<ResponseWaitExpired>(error).is_some()
}

pub(crate) fn find_source<T: std::error::Error + 'static>(error: &anyhow::Error) -> Option<&T> {
    error.chain().find_map(|cause| {
        cause.downcast_ref::<T>().or_else(|| {
            cause
                .downcast_ref::<MutationFailure>()?
                .cause
                .chain()
                .find_map(|inner| inner.downcast_ref::<T>())
        })
    })
}

#[derive(Clone, Copy)]
pub(crate) struct StopRequest {
    pub at: Instant,
    pub reason: StopReason,
}

pub(crate) fn check_start(job: Option<&Job>) -> anyhow::Result<()> {
    if job.is_some_and(|job| job.stop().is_some()) {
        return Err(Stopped.into());
    }
    Ok(())
}

pub(crate) fn check_dispatch(job: Option<&Job>, command: &str) -> anyhow::Result<()> {
    if let Some(job) = job {
        job.dispatch_gate(command);
    }
    check_start(job)
}

pub(crate) struct Session {
    pub id: SessionId,
    pub channel: DeviceChannel,
    pub target: Target,
    pub digest: bool,
}

pub(crate) struct SessionChannel<'a> {
    pub inner: &'a mut DeviceChannel,
    pub job: &'a Job,
}

impl Read for SessionChannel<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.inner.read(out)
    }
}
impl BufRead for SessionChannel<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, count: usize) {
        self.inner.consume(count);
    }
}
impl Write for SessionChannel<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl QdlChan for SessionChannel<'_> {
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
        if self
            .job
            .stop()
            .is_some_and(|stop| self.job.now().saturating_duration_since(stop.at) >= RESPONSE_GRACE)
        {
            return Err(ResponseWaitExpired.into());
        }
        self.inner.check_response_wait()
    }
}
