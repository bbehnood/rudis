use std::thread::current;

use crate::{Command, RespValue, store::Store};

impl Store {
    pub fn execute(&mut self, command: Command) -> RespValue {
        match command {
            Command::Ping => RespValue::SimpleString("PONG".into()),

            Command::Echo(msg) => RespValue::BulkString(Some(msg)),

            Command::Get(key) => {
                RespValue::BulkString(self.get(&key).map(<[u8]>::to_vec))
            },

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

            Command::Incr(key) => {
                let current = match self.get(&key) {
                    Some(bytes) => {
                        let parsed = std::str::from_utf8(bytes)
                            .ok()
                            .and_then(|s| s.parse::<i64>().ok());

                        match parsed {
                            Some(n) => n,

                            None => return RespValue::Error(
                                "ERR value is not an integer or out of range"
                                    .into(),
                            ),
                        }
                    },

                    None => 0,
                };

                match current.checked_add(1) {
                    Some(next) => {
                        self.set(key, next.to_string().into_bytes());
                        RespValue::Integer(next)
                    },

                    None => RespValue::Error(
                        "ERR increment or decrement would overflow".into(),
                    ),
                }
            },
        }
    }
}
