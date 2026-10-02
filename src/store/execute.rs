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

                // TODO: Remove explicit cast to i64 as it can wrap around
                // on a 64-bit target
                RespValue::Integer(deleted as i64)
            },

            Command::Exists(keys) => {
                let count = keys.iter().filter(|key| self.exists(key)).count();

                RespValue::Integer(count as i64)
            },

            Command::Incr(_key) => {
                todo!()
            },
        }
    }
}
