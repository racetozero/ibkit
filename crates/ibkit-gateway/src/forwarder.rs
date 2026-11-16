#![forbid(unsafe_code)]
use anyhow::Result;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::{sleep, timeout},
};

/// Allowed client sources: (network, prefix length) pairs; `None` allows
/// every source (`Config::allowed_sources`).
pub type Sources = Option<Vec<(IpAddr, u8)>>;

/// Whether `address` is inside one of `sources`.
pub fn allowed(sources: &Sources, address: IpAddr) -> bool {
    let Some(sources) = sources else {
        return true;
    };
    let address = match address {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
        v4 => v4,
    };
    sources
        .iter()
        .any(|(network, prefix)| match (network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(*prefix)).unwrap_or(0);
                u32::from(*network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(*prefix)).unwrap_or(0);
                u128::from(*network) & mask == u128::from(address) & mask
            }
            _ => false,
        })
}

/// Forward `listen` to Gateway's API port on loopback until the task is
/// aborted, for clients inside `sources`. Gateway sees every forwarded
/// client as 127.0.0.1, so its own trusted-IP list does not apply here.
/// The listener is bound again 500 ms after a failure; only the first
/// failure of a series is logged.
pub async fn supervise(listen: SocketAddr, api_port: u16, sources: Sources) {
    let target = SocketAddr::from(([127, 0, 0, 1], api_port));
    let mut reported = false;
    loop {
        match TcpListener::bind(listen).await {
            Ok(listener) => {
                reported = false;
                if let Err(error) = forward_api(listener, target, &sources).await {
                    eprintln!(
                        "The API forwarder stopped ({error}); the controller opens its listener again."
                    );
                }
            }
            Err(error) if !reported => {
                reported = true;
                eprintln!(
                    "Cannot listen on api_listen {listen}: {error}. Free the address or change api_listen; the controller tries again every 500 ms."
                );
            }
            Err(_) => {}
        }
        sleep(Duration::from_millis(500)).await;
    }
}

/// Forward an explicit external listener to Gateway's loopback socket.
/// The caller owns this future; dropping it aborts and drops all connections.
/// An accept error (too many open files, an aborted handshake) is logged and
/// the live connections continue.
pub async fn forward_api(
    listener: TcpListener,
    target: SocketAddr,
    sources: &Sources,
) -> Result<()> {
    let permits = Arc::new(Semaphore::new(64));
    let mut connections = JoinSet::new();
    let mut refused = std::collections::HashSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut incoming, peer) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        eprintln!("The API forwarder could not accept a connection: {error}");
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                if !allowed(sources, peer.ip()) {
                    // Each source is reported once; a scan of many addresses
                    // must not grow the set without a bound.
                    if refused.len() >= 1024 {
                        refused.clear();
                    }
                    if refused.insert(peer.ip()) {
                        eprintln!(
                            "The API forwarder refused {}: it is not in api_allowed_sources. Add the address or range to api_allowed_sources to allow it.",
                            peer.ip()
                        );
                    }
                    continue;
                }
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue; };
                connections.spawn(async move {
                    let _permit = permit;
                    let Ok(Ok(mut gateway)) = timeout(Duration::from_secs(3), TcpStream::connect(target)).await else { return; };
                    // IB API messages are small; do not hold them for Nagle.
                    let _ = incoming.set_nodelay(true);
                    let _ = gateway.set_nodelay(true);
                    let _ = tokio::io::copy_bidirectional(&mut incoming, &mut gateway).await;
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}
