use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bitfield {
	data: Vec<u8>
}

impl Bitfield {
    pub fn new(length: usize) -> Bitfield {
        Bitfield {
            data: vec![0; if length % 8 == 0 { length / 8 } else { length / 8 + 1 }]
        }
    }

    pub fn from_bytes(data: &[u8]) -> Bitfield {
        Bitfield {
            data: data.to_vec()
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    /// Get a bit from the bitfield
    pub fn get(&self, i: usize) -> bool {
        self.data[i / 8] & (1 << i % 8) != 0
    }

    /// Set a bit in the bitfield
    pub fn set(&mut self, i: usize, value: bool) {
        if value {
            self.data[i / 8] = self.data[i / 8] | (1 << i % 8);
        } else {
            self.data[i / 8] = self.data[i / 8] & (0b11111111 ^ (1 << i % 8));
        }
    }
}
