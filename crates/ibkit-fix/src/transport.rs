//! Async transport that drives a [`Session`] over TCP or TLS.

use crate::message::{FrameSplitter, Message, fixcomp};
use crate::session::{Output, SeqStore, Session, SessionConfig, SessionEvent};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
pub use tokio_rustls::rustls;

/// Something the connection reports.
#[derive(Clone, Debug, PartialEq)]
pub enum FixEvent {
    App(Message),
    Session(SessionEvent),
    Disconnected(String),
}

enum Command {
    Send(Message),
    Logout(String),
}

/// Handle to a running FIX connection.
#[derive(Clone)]
pub struct FixHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl FixHandle {
    /// Send an application message (sequence number and header are added).
    pub fn send(&self, msg: Message) -> Result<(), FixClosed> {
        self.tx.send(Command::Send(msg)).map_err(|_| FixClosed)
    }

    pub fn logout(&self, text: &str) -> Result<(), FixClosed> {
        self.tx
            .send(Command::Logout(text.into()))
            .map_err(|_| FixClosed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the FIX connection is closed")]
pub struct FixClosed;

/// Run a session over an established stream (TCP, TLS or any other byte
/// stream). Returns a handle and the event stream. For an initiator the
/// Logon is sent immediately.
pub fn run<S>(
    stream: S,
    cfg: SessionConfig,
    store: Box<dyn SeqStore>,
) -> (FixHandle, mpsc::UnboundedReceiver<FixEvent>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<Command>();
    let (etx, erx) = mpsc::unbounded_channel::<FixEvent>();
    tokio::spawn(async move {
        let acceptor = cfg.acceptor;
        let compression = cfg.compression;
        let wire = |bytes: Vec<u8>| if compression { fixcomp(&bytes) } else { bytes };
        let mut session = Session::new(cfg, store);
        let (mut rd, mut wr) = tokio::io::split(stream);
        if !acceptor {
            let bytes = session.logon(Instant::now());
            if wr.write_all(&wire(bytes)).await.is_err() {
                let _ = etx.send(FixEvent::Disconnected("could not send Logon".into()));
                return;
            }
        }
        let mut splitter = FrameSplitter::default();
        let mut buf = vec![0u8; 64 * 1024];
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        let reason = loop {
            let outputs = tokio::select! {
                n = rd.read(&mut buf) => match n {
                    Ok(0) => break "the counterparty closed the connection".to_string(),
                    Err(e) => break format!("socket read failed: {e}"),
                    Ok(n) => {
                        splitter.extend(&buf[..n]);
                        let mut outs = Vec::new();
                        while let Some(raw) = splitter.next_message() {
                            outs.extend(session.on_raw(&raw, Instant::now()));
                        }
                        outs
                    }
                },
                cmd = rx.recv() => match cmd {
                    Some(Command::Send(m)) => vec![Output::Send(session.send(m, Instant::now()))],
                    Some(Command::Logout(t)) => vec![Output::Send(session.logout(&t, Instant::now()))],
                    None => break "the handle was dropped".to_string(),
                },
                _ = tick.tick() => session.on_timer(Instant::now()),
            };
            // Messages produced together go out in one write (one FIXCOMP
            // block when compression is on), before a disconnect.
            let mut pending = Vec::new();
            let mut disconnect = None;
            for o in outputs {
                match o {
                    Output::Send(bytes) => pending.extend(bytes),
                    Output::App(m) => {
                        let _ = etx.send(FixEvent::App(m));
                    }
                    Output::Event(e) => {
                        let _ = etx.send(FixEvent::Session(e));
                    }
                    Output::Disconnect(r) => {
                        disconnect = Some(r);
                        break;
                    }
                }
            }
            if !pending.is_empty()
                && let Err(e) = wr.write_all(&wire(pending)).await
            {
                break format!("socket write failed: {e}");
            }
            if let Some(r) = disconnect {
                break r;
            }
        };
        let _ = wr.shutdown().await;
        let _ = etx.send(FixEvent::Disconnected(reason));
    });
    (FixHandle { tx }, erx)
}

/// Connect to `addr` and start an initiator session.
pub async fn connect(
    addr: &str,
    cfg: SessionConfig,
    store: Box<dyn SeqStore>,
) -> std::io::Result<(FixHandle, mpsc::UnboundedReceiver<FixEvent>)> {
    let stream = TcpStream::connect(addr).await?;
    let _ = stream.set_nodelay(true);
    Ok(run(stream, cfg, store))
}

/// Connect to `addr` over TLS and start an initiator session. `server_name`
/// is the name the server certificate must carry; `tls` holds the trusted
/// roots (for example built from the certificate IB or stunnel presents).
pub async fn connect_tls(
    addr: &str,
    server_name: &str,
    tls: Arc<rustls::ClientConfig>,
    cfg: SessionConfig,
    store: Box<dyn SeqStore>,
) -> std::io::Result<(FixHandle, mpsc::UnboundedReceiver<FixEvent>)> {
    let name = rustls::pki_types::ServerName::try_from(server_name.to_string()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("TLS server name {server_name:?} is not a valid DNS name or IP address ({e}); pass the host name in the server certificate"),
        )
    })?;
    let tcp = TcpStream::connect(addr).await?;
    let _ = tcp.set_nodelay(true);
    let stream = tokio_rustls::TlsConnector::from(tls)
        .connect(name, tcp)
        .await?;
    Ok(run(stream, cfg, store))
}
