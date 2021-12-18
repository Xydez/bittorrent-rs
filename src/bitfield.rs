use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Bitfield {
    // length: usize,
	data: Vec<u8>
}

enum BitfieldError {
    IndexOutOfBounds
}

type Result<T> = std::result::Result<T, BitfieldError>;

impl Bitfield {
    pub fn new(length: usize) -> Bitfield {
        Bitfield {
            // length,
            data: vec![0; if length % 8 == 0 { length / 8 } else { length / 8 + 1 }]
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Bitfield {
        Bitfield {
            data
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    /// Get a bit from the bitfield
    pub fn get(&self, i: usize) -> Result<bool> {
        // if i >= self.length {
            // Err(BitfieldError::IndexOutOfBounds)
        // } else {
            Ok(self.data[i / 8] & (1 << i % 8) != 0)
        // }
    }

    /// Set a bit in the bitfield
    pub fn set(&mut self, i: usize, value: bool) -> Result<()> {
        // if i >= self.length {
        //     Err(BitfieldError::IndexOutOfBounds)
        // } else {
            if value {
                self.data[i / 8] = self.data[i / 8] | (1 << i % 8);
            } else {
                self.data[i / 8] = self.data[i / 8] & (0b11111111 ^ (1 << i % 8));
            }
            
            Ok(())
        // }
    }


    // /// Returns the bits the bitfield can contain
    // pub fn len(&self) -> usize {
    //     self.length
    // }
}
