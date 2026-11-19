use crate::{device, model::Backend};
use anyhow::Result;
use qdl::types::{FirehoseConfiguration, QdlDevice, QdlReadWrite};
use std::{io, sync::Arc};

pub type DeviceChannel = QdlDevice<dyn QdlReadWrite>;
pub type Task = Box<dyn FnOnce() + Send>;
type Enumerate = Arc<dyn Fn() -> Result<Vec<String>> + Send + Sync>;
type Open = Arc<dyn Fn(&str, FirehoseConfiguration) -> Result<DeviceChannel> + Send + Sync>;
type Spawn = Arc<dyn Fn(Task) -> io::Result<()> + Send + Sync>;
#[cfg(feature = "cloud")]
type CollectEvidence = Arc<dyn Fn() -> Result<serde_json::Value> + Send + Sync>;

#[derive(Clone)]
struct Provider {
    enumerate: Enumerate,
    open: Open,
}

#[derive(Clone)]
pub struct HostServices {
    usb: Option<Provider>,
    serial: Option<Provider>,
    spawn: Spawn,
    #[cfg(feature = "cloud")]
    collect_evidence: CollectEvidence,
    #[cfg(all(feature = "cloud", any(test, feature = "test-support")))]
    loopback: Option<std::net::SocketAddr>,
    #[cfg(all(test, feature = "cloud"))]
    pub(crate) authenticated_asset_fixture: bool,
}

impl Default for HostServices {
    fn default() -> Self {
        Self {
            usb: None,
            serial: None,
            #[cfg(feature = "cloud")]
            collect_evidence: Arc::new(crate::swm::hardware_evidence),
            #[cfg(all(feature = "cloud", any(test, feature = "test-support")))]
            loopback: None,
            #[cfg(all(test, feature = "cloud"))]
            authenticated_asset_fixture: false,
            spawn: Arc::new(|task| {
                std::thread::Builder::new()
                    .name("oplus-edl-worker".into())
                    .spawn(task)
                    .map(drop)
            }),
        }
    }
}

impl HostServices {
    #[cfg(all(feature = "cloud", any(test, feature = "test-support")))]
    pub(crate) fn with_loopback(mut self, address: std::net::SocketAddr) -> anyhow::Result<Self> {
        anyhow::ensure!(
            address.ip().is_loopback() && address.port() != 0,
            "a loopback IP and nonzero port are required"
        );
        self.loopback = Some(address);
        Ok(self)
    }

    #[cfg(feature = "cloud")]
    pub(crate) fn cloud_loopback(&self) -> Option<std::net::SocketAddr> {
        #[cfg(any(test, feature = "test-support"))]
        return self.loopback;
        #[cfg(not(any(test, feature = "test-support")))]
        None
    }

    pub fn native() -> Self {
        let mut host = Self::default();
        for (backend, enabled) in [
            (Backend::Usb, cfg!(feature = "usb")),
            (Backend::Serial, cfg!(feature = "serial")),
        ] {
            if enabled {
                host = host.with_backend(
                    backend,
                    move || device::candidates(backend),
                    move |key, cfg| device::open(backend, key, cfg),
                );
            }
        }
        host
    }

    pub fn with_backend(
        mut self,
        backend: Backend,
        enumerate: impl Fn() -> Result<Vec<String>> + Send + Sync + 'static,
        open: impl Fn(&str, FirehoseConfiguration) -> Result<DeviceChannel> + Send + Sync + 'static,
    ) -> Self {
        let provider = Some(Provider {
            enumerate: Arc::new(enumerate),
            open: Arc::new(open),
        });
        match backend {
            Backend::Usb => self.usb = provider,
            Backend::Serial => self.serial = provider,
        }
        self
    }

    pub fn with_scheduler(
        mut self,
        spawn: impl Fn(Task) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.spawn = Arc::new(spawn);
        self
    }

    #[cfg(feature = "cloud")]
    pub fn with_host_evidence(
        mut self,
        collect: impl Fn() -> Result<serde_json::Value> + Send + Sync + 'static,
    ) -> Self {
        self.collect_evidence = Arc::new(collect);
        self
    }

    #[cfg(feature = "cloud")]
    pub(crate) fn collect_host_evidence(&self) -> Result<serde_json::Value> {
        (self.collect_evidence)()
    }

    fn provider(&self, backend: Backend) -> Option<&Provider> {
        match backend {
            Backend::Usb => self.usb.as_ref(),
            Backend::Serial => self.serial.as_ref(),
        }
    }

    pub(crate) fn supports(&self, backend: Backend) -> bool {
        self.provider(backend).is_some()
    }

    pub(crate) fn enumerate(&self, backend: Backend) -> Result<Vec<String>> {
        (self.provider(backend).expect("validated backend").enumerate)()
    }

    pub(crate) fn open(
        &self,
        backend: Backend,
        key: &str,
        config: FirehoseConfiguration,
    ) -> Result<DeviceChannel> {
        (self.provider(backend).expect("validated backend").open)(key, config)
    }

    pub(crate) fn spawn(&self, task: Task) -> io::Result<()> {
        (self.spawn)(task)
    }
}
