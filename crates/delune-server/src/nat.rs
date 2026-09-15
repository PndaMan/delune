//! Opening the Soulseek port on the router with UPnP, when people turn it on.
//!
//! Soulseek works best when other users can connect to delune directly: more of
//! them answer searches, and uploads start without a round trip through the
//! server. Most home routers let a program ask for a port to be forwarded (UPnP
//! IGD). delune asks for an hour at a time and renews every half hour, and removes
//! the mapping when the setting is turned off. It's off by default, since it
//! changes the router's configuration.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use delune_core::api::{PortMapping, PortMappingState};
use igd_next::{PortMappingProtocol, SearchOptions};

use crate::AppState;

const LEASE: Duration = Duration::from_secs(60 * 60);
const RENEW_EVERY: Duration = Duration::from_secs(30 * 60);
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(10 * 60);
const DESCRIPTION: &str = "delune (Soulseek)";

#[derive(Debug, Default)]
pub struct Nat {
    status: Mutex<Option<PortMapping>>,
}

impl Nat {
    #[must_use]
    pub fn status(&self) -> PortMapping {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone().unwrap_or(PortMapping::off())
    }

    fn set(&self, state: PortMappingState, external_ip: Option<IpAddr>, message: Option<String>) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(PortMapping { state, external_ip: external_ip.map(|ip| ip.to_string()), message });
    }
}

/// Keep the port mapped while sharing settings ask for it. Call once at startup.
pub fn start(app: &AppState) {
    let Some(port) = app.soulseek_port else { return };
    let app = app.clone();
    tokio::spawn(async move {
        let mut mapped = false;
        loop {
            let wanted = app.sharing.settings().upnp;
            let wait = if wanted {
                match map(port).await {
                    Ok(external_ip) => {
                        if !mapped {
                            tracing::info!(port, %external_ip, "Soulseek port opened on the router");
                        }
                        mapped = true;
                        app.nat.set(PortMappingState::Mapped, Some(external_ip), None);
                        RENEW_EVERY
                    }
                    Err(message) => {
                        tracing::warn!(port, %message, "couldn't open the Soulseek port on the router");
                        mapped = false;
                        app.nat.set(PortMappingState::Failed, None, Some(message));
                        RETRY_AFTER_FAILURE
                    }
                }
            } else {
                if mapped {
                    unmap(port).await;
                    mapped = false;
                }
                app.nat.set(PortMappingState::Off, None, None);
                RENEW_EVERY
            };
            // Wake early when the setting changes.
            let mut changes = app.nat_wake.subscribe();
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                _ = changes.changed() => {}
            }
        }
    });
}

async fn map(port: u16) -> Result<IpAddr, String> {
    let options = SearchOptions { timeout: Some(Duration::from_secs(5)), ..SearchOptions::default() };
    let gateway = igd_next::aio::tokio::search_gateway(options)
        .await
        .map_err(|_| "No router answered. UPnP may be turned off on it.".to_owned())?;
    let local = local_address_towards(gateway.addr).ok_or("Couldn't tell which address the router sees delune at.")?;
    let lease = u32::try_from(LEASE.as_secs()).unwrap_or(u32::MAX);
    gateway
        .add_port(PortMappingProtocol::TCP, port, SocketAddr::new(local, port), lease, DESCRIPTION)
        .await
        .map_err(|e| format!("The router refused to forward port {port}: {e}"))?;
    gateway.get_external_ip().await.map_err(|e| format!("Forwarded, but the router didn't say its address: {e}"))
}

async fn unmap(port: u16) {
    let options = SearchOptions { timeout: Some(Duration::from_secs(5)), ..SearchOptions::default() };
    if let Ok(gateway) = igd_next::aio::tokio::search_gateway(options).await {
        match gateway.remove_port(PortMappingProtocol::TCP, port).await {
            Ok(()) => tracing::info!(port, "Soulseek port closed on the router"),
            Err(error) => tracing::warn!(port, %error, "couldn't remove the router's port mapping"),
        }
    }
}

/// The local address the operating system would use to reach `gateway`.
fn local_address_towards(gateway: SocketAddr) -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    // Connecting a UDP socket sends nothing; it just picks a route.
    socket.connect(gateway).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asks the local network for a router, without mapping anything.
    #[tokio::test]
    #[ignore = "needs a UPnP router on the network"]
    async fn finds_the_router() {
        let options = SearchOptions { timeout: Some(Duration::from_secs(5)), ..SearchOptions::default() };
        let gateway = igd_next::aio::tokio::search_gateway(options).await.unwrap();
        println!("router at {}, delune at {:?}", gateway.addr, local_address_towards(gateway.addr));
        println!("public address {:?}", gateway.get_external_ip().await);
    }

    #[test]
    fn picks_the_route_towards_the_gateway() {
        assert_eq!(local_address_towards("127.0.0.1:1900".parse().unwrap()), Some("127.0.0.1".parse().unwrap()));
    }
}
