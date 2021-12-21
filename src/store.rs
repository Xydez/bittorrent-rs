pub trait Store: std::fmt::Debug + Send {
	fn get(&self, begin: usize, length: usize) -> &[u8];
	fn set(&mut self, begin: usize, data: &[u8]);
}

#[derive(Debug)]
pub struct MemoryStore {
	data: Vec<u8>
}

impl MemoryStore {
	pub fn new(length: usize) -> MemoryStore {
		MemoryStore {
			data: vec![0; length]
		}
	}
}

impl Store for MemoryStore {
    fn get(&self, i: usize, length: usize) -> &[u8] {
        return &self.data[i..(i + length)];
    }

    fn set(&mut self, begin: usize, data: &[u8]) {
        for i in 0..data.len() {
			self.data[begin + i] = data[i];
		}
    }
}

// TODO: FileStore
