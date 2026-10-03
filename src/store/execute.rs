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

            Command::Incr(_key) => {
                todo!()
            },
        }
    }
}
