use crate::{
    CoreService, HostServices,
    model::{PartitionSnapshot, Scope, SessionId},
    service::Clock,
};

pub fn service_with_stop_clock(host: HostServices, clock: Clock) -> CoreService {
    CoreService::with_control(host, clock, None)
}

#[cfg(feature = "cloud")]
pub fn loopback_service(
    host: HostServices,
    address: std::net::SocketAddr,
) -> anyhow::Result<CoreService> {
    Ok(CoreService::new(host.with_loopback(address)?))
}

pub fn service_with_lun_query_gate(
    host: HostServices,
    clock: Clock,
    gate: Box<dyn FnOnce() + Send>,
) -> CoreService {
    CoreService::with_control(host, clock, Some(gate))
}

pub fn service_with_dispatch_gate(
    host: HostServices,
    clock: Clock,
    command: &'static str,
    gate: Box<dyn FnOnce() + Send>,
) -> CoreService {
    let service = service_with_stop_clock(host, clock);
    set_dispatch_gate(&service, command, gate);
    service
}

pub fn set_dispatch_gate(
    service: &CoreService,
    command: &'static str,
    gate: Box<dyn FnOnce() + Send>,
) {
    service.set_dispatch_gate(command, gate);
}

pub fn committed_partition_snapshot(
    service: &CoreService,
    session: SessionId,
    scope: &Scope,
) -> Option<PartitionSnapshot> {
    service.committed_snapshot(session, scope)
}
