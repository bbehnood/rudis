pub mod command;
pub mod protocol;
pub mod store;

pub use command::{Command, CommandError};
pub use protocol::{ParseError, RespParser, RespValue};
pub use store::Store;

use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{Instrument, debug, error, info, info_span, trace, warn};

use crate::protocol::MAX_BULK_LEN;

pub struct Server {
    listener: TcpListener,
    store: Arc<Mutex<Store>>,
}

const BUFFER_SIZE: usize = 4096;
const MAX_BUFFERED: usize = MAX_BULK_LEN + (64 * 1024);
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
const MAX_LOGGED_LEN: usize = 128;

impl Server {
    pub async fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;

        info!(addr = %listener.local_addr()?, "listening for connections");

        Ok(Self { listener, store: Arc::new(Mutex::new(Store::default())) })
    }

    pub async fn run(self) -> io::Result<()> {
        loop {
            let (stream, addr) = match self.listener.accept().await {
                Ok(conn) => conn,

                Err(e) if is_connection_error(&e) => {
                    debug!(error = %e, "connection dropped before accept");
                    continue;
                },

                Err(e) => {
                    error!(
                        error = %e,
                        backoff = ?ACCEPT_BACKOFF,
                        "failed to accept connection, retrying"
                    );
                    tokio::time::sleep(ACCEPT_BACKOFF).await;
                    continue;
                },
            };

            let store = Arc::clone(&self.store);
            let span = info_span!("conn", peer = %addr);

            tokio::spawn(
                async move {
                    debug!("client connected");

                    let started = Instant::now();

                    match handle_connection(stream, store).await {
                        Ok(()) => {
                            debug!(
                                elapsed = ?started.elapsed(),
                                "client disconnected"
                            );
                        },

                        Err(e) if is_peer_gone(&e) => {
                            debug!(
                                error = %e,
                                elapsed = ?started.elapsed(),
                                "client connection lost"
                            );
                        },

                        Err(e) => {
                            warn!(
                                error = %e,
                                kind = ?e.kind(),
                                elapsed = ?started.elapsed(),
                                "connection failed"
                            );
                        },
                    }
                }
                .instrument(span),
            );
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    store: Arc<Mutex<Store>>,
) -> io::Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(BUFFER_SIZE);
    let mut out: Vec<u8> = Vec::with_capacity(BUFFER_SIZE);

    loop {
        let n = stream.read_buf(&mut buf).await?;
        if n == 0 {
            if !buf.is_empty() {
                debug!(
                    discarded = buf.len(),
                    "client closed connection mid-frame"
                );
            }

            return Ok(());
        }

        trace!(bytes = n, buffered = buf.len(), "read from socket");

        let mut consumed = 0;
        let mut fatal: Option<ParseError> = None;

        loop {
            let mut parser = RespParser::new(&buf[consumed..]);

            match parser.parse() {
                Ok((value, used)) => {
                    consumed += used;
                    let reply = dispatch(value, &store);
                    append_reply(&reply, &mut out);
                },

                Err(ParseError::Incomplete) => break,

                Err(e) => {
                    fatal = Some(e);
                    break;
                },
            }
        }

        buf.drain(..consumed);

        if fatal.is_none() && buf.len() > MAX_BUFFERED {
            fatal = Some(ParseError::TooLarge);
        }

        if let Some(e) = &fatal {
            warn!(
                error = %e,
                buffered = buf.len(),
                "protocol error, closing connection"
            );

            let reply = RespValue::Error(format!("ERR Protocol error: {e}"));
            append_reply(&reply, &mut out);
        }

        if !out.is_empty() {
            trace!(bytes = out.len(), "writing replies");

            stream.write_all(&out).await?;
            out.clear();
        }

        if fatal.is_some() {
            return Ok(());
        }
    }
}

fn dispatch(value: RespValue, store: &Mutex<Store>) -> RespValue {
    match Command::from_resp(value) {
        Ok(command) => {
            trace!(command = command.name(), "executing command");

            let mut store = store.lock().unwrap_or_else(|poisoned| {
                error!(
                    "store mutex poisoned by a panic in another connection, \
                     recovering"
                );

                store.clear_poison();
                poisoned.into_inner()
            });
            store.execute(command)
        },

        Err(e) => {
            let msg = e.to_string();

            debug!(
                error = %truncate(&msg).escape_debug(),
                "rejected invalid command"
            );

            let msg = msg.replace(['\r', '\n'], " ");
            RespValue::Error(format!("ERR {msg}"))
        },
    }
}

fn append_reply(reply: &RespValue, out: &mut Vec<u8>) {
    if let Err(e) = reply.encode_into(out) {
        error!(error = %e, "failed to encode reply, sending internal error");

        let _ = RespValue::Error("ERR internal error".into()).encode_into(out);
    }
}

fn truncate(s: &str) -> &str {
    match s.char_indices().nth(MAX_LOGGED_LEN) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn is_peer_gone(e: &io::Error) -> bool {
    is_connection_error(e) || e.kind() == io::ErrorKind::BrokenPipe
}

fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
    )
}
