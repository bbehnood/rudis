pub mod command;
pub mod protocol;
pub mod store;

pub use command::{Command, CommandError};
pub use protocol::{ParseError, RespParser, RespValue};
pub use store::Store;

use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::protocol::MAX_BULK_LEN;

pub struct Server {
    listener: TcpListener,
    store: Arc<Mutex<Store>>,
}

const BUFFER_SIZE: usize = 4096;
const MAX_BUFFERED: usize = MAX_BULK_LEN + (64 * 1024);

impl Server {
    pub async fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;

        Ok(Self { listener, store: Arc::new(Mutex::new(Store::default())) })
    }

    pub async fn run(self) -> io::Result<()> {
        loop {
            let (stream, addr) = self.listener.accept().await?;

            println!("client connected: {addr}");

            let store = Arc::clone(&self.store);

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, store).await {
                    eprintln!("connection error: {e}");
                }

                println!("client disconnected: {addr}");
            });
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
            return Ok(());
        }

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
            let reply = RespValue::Error(format!("ERR Protocol error: {e}"));
            append_reply(&reply, &mut out);
        }

        if !out.is_empty() {
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
            let mut store =
                store.lock().unwrap_or_else(PoisonError::into_inner);
            store.execute(command)
        },

        Err(e) => {
            let msg = e.to_string().replace(['\r', '\n'], " ");
            RespValue::Error(format!("ERR {msg}"))
        },
    }
}

fn append_reply(reply: &RespValue, out: &mut Vec<u8>) {
    if reply.encode_into(out).is_err() {
        let _ = RespValue::Error("ERR internal error".into()).encode_into(out);
    }
}
