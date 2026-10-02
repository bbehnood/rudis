use crate::{Command, RespValue, store::Store};

impl Store {
    pub fn execute(&mut self, command: Command) -> RespValue {
        match command {
            Command::Ping => RespValue::SimpleString("PONG".into()),

            Command::Echo(msg) => RespValue::BulkString(Some(msg)),

            Command::Get(key) => {
                RespValue::BulkString(self.get(&key).map(|val| val.to_vec()))
            },

            Command::Set { key, value } => {
                self.set(key, value);
                RespValue::SimpleString("OK".into())
            },

            Command::Del(keys) => {
                let deleted = keys.iter().filter(|key| self.del(key)).count();

                RespValue::Integer(deleted as i64)
            },

            Command::Exists(keys) => {
                let count = keys.iter().filter(|key| self.exists(key)).count();

                RespValue::Integer(count as i64)
            },

            Command::Incr(key) => {
                todo!()
            },
        }
    }
}
