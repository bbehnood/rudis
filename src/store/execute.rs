use std::collections::hash_map::Entry;

use bytes::Bytes;
use thiserror::Error;
use tracing::debug;

use crate::{Command, RespValue, store::Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ExecuteError {
    #[error("value is not an integer or out of range")]
    NotInteger,

    #[error("increment or decrement would overflow")]
    Overflow,
}

impl Store {
    pub fn execute(&mut self, command: Command) -> RespValue {
        match command {
            Command::Ping => RespValue::SimpleString("PONG".into()),

            Command::Echo(msg) => RespValue::BulkString(Some(msg)),

            Command::Get(key) => RespValue::BulkString(self.get(&key)),

            Command::Set { key, value } => {
                self.set(key, value);
                RespValue::SimpleString("OK".into())
            },

            Command::Del(keys) => {
                let deleted = keys.iter().filter(|key| self.del(key)).count();

                RespValue::Integer(i64::try_from(deleted).unwrap_or(i64::MAX))
            },

            Command::Exists(keys) => {
                let count = keys.iter().filter(|key| self.exists(key)).count();

                RespValue::Integer(i64::try_from(count).unwrap_or(i64::MAX))
            },

            Command::Incr(key) => match self.incr(key) {
                Ok(n) => RespValue::Integer(n),
                Err(e) => {
                    debug!(error = %e, "INCR failed");
                    RespValue::Error(format!("ERR {e}"))
                },
            },
        }
    }

    fn incr(&mut self, key: Bytes) -> Result<i64, ExecuteError> {
        match self.data.entry(key) {
            Entry::Occupied(mut entry) => {
                let current = parse_redis_i64(entry.get())
                    .ok_or(ExecuteError::NotInteger)?;

                let next =
                    current.checked_add(1).ok_or(ExecuteError::Overflow)?;

                *entry.get_mut() = Bytes::from(next.to_string().into_bytes());

                Ok(next)
            },

            Entry::Vacant(entry) => {
                entry.insert(Bytes::from_static(b"1"));

                Ok(1)
            },
        }
    }
}

fn parse_redis_i64(bytes: &[u8]) -> Option<i64> {
    let digits = bytes.strip_prefix(b"-").unwrap_or(bytes);

    let canonical = match digits {
        [b'0'] => bytes.len() == 1,

        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),

        _ => false,
    };

    if !canonical {
        return None;
    }

    std::str::from_utf8(bytes).ok()?.parse().ok()
}
