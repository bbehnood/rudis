use std::collections::HashMap;

use bytes::Bytes;

mod execute;

pub struct Store {
    data: HashMap<Bytes, Bytes>,
}

impl Store {
    #[must_use]
    pub fn new() -> Self {
        Self { data: HashMap::new() }
    }

    pub fn get(&self, key: &[u8]) -> Option<Bytes> {
        self.data.get(key).cloned()
    }

    pub fn set(&mut self, key: Bytes, value: Bytes) {
        self.data.insert(key, value);
    }

    pub fn del(&mut self, key: &[u8]) -> bool {
        self.data.remove(key).is_some()
    }

    #[must_use]
    pub fn exists(&self, key: &[u8]) -> bool {
        self.data.contains_key(key)
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}
