use std::collections::HashMap;

pub struct Store {
    data: HashMap<Vec<u8>, Vec<u8>>,
}

impl Store {
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        self.data.get(key).map(Vec::as_slice)
    }

    pub fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.data.insert(key, value);
    }

    pub fn del(&mut self, key: &[u8]) -> bool {
        self.data.remove(key).is_some()
    }

    pub fn exists(&self, key: &[u8]) -> bool {
        self.data.contains_key(key)
    }
}
