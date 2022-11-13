use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bitfield {
    data: Vec<u8>,
    length: usize
}

impl Bitfield {
    pub fn new(length: usize) -> Bitfield {
        Bitfield {
            data: vec![0; length.div_ceil(8)],
            length
        }
    }

    // TODO: `impl From<&[u8]> for Bitfield` instead
    pub fn from_bytes(data: &[u8]) -> Bitfield {
        Bitfield {
            data: data.to_vec(),
            length: data.len() * 8 // TODO: This may be incorrect
        }
    }

    pub fn from_bytes_length(data: &[u8], length: usize) -> Bitfield {
        assert!(data.len() * 8 > length && data.len() * 8 - length < 8, "Length out of bounds (data.len() * 8 > length && data.len() * 8 - length < 8)");

        Bitfield {
            data: data.to_vec(),
            length
        }
    }

    // TODO: `impl From<Bitfield> for Vec<u8>`
    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    /// Get a bit from the bitfield
    pub fn get(&self, i: usize) -> bool {
        self.data[i / 8] & (1 << (7 - (i % 8))) != 0
    }

    /// Set a bit in the bitfield
    pub fn set(&mut self, i: usize, value: bool) {
        if value {
            self.data[i / 8] |= 1 << (7 - (i % 8));
        } else {
            self.data[i / 8] &= 0b11111111 ^ (1 << (7 - (i % 8)));
        }
    }

    /// Iterator over all bits in the bitfield
    pub fn iter(&self) -> BitfieldIterator {
        BitfieldIterator {
            bitfield: self,
            index: 0
        }
    }

    /// Get the number of bits in the bitfield
    pub fn len(&self) -> usize {
        self.length
    }

    /// Returns true if the bitfield is empty
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

pub struct BitfieldIterator<'a> {
    bitfield: &'a Bitfield,
    index: usize
}

impl<'a> Iterator for BitfieldIterator<'a> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        let item = if self.index >= self.bitfield.len() {
            None
        } else {
            Some(self.bitfield.get(self.index))
        };

        self.index += 1;

        item
    }
}

impl<'a> IntoIterator for &'a Bitfield {
    type Item = bool;
    type IntoIter = BitfieldIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        BitfieldIterator {
            bitfield: self,
            index: 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create() {
        Bitfield::new(42);
        Bitfield::from_bytes(&[
            0b01010101,
            0b10101010
        ]);
    }

    #[test]
    fn test_len() {
        assert_eq!(Bitfield::new(42).len(), 42);
        assert_eq!(Bitfield::from_bytes(&[
            0b01010101,
            0b10101010
        ]).len(), 16);
        assert_eq!(Bitfield::from_bytes_length(&[
            0b01010101,
            0b10101010
        ], 12).len(), 12);
    }

    #[test]
    fn test_get() {
        let bitfield = Bitfield::from_bytes_length(
            &[
                0b01010101,
                0b10101010
            ],
            12
        );

        for i in 0..12 {
            let expected = i % 2 == if i < 8 { 1 } else { 0 };
            assert_eq!(bitfield.get(i), expected);
        }
    }

    #[test]
    fn test_iterator() {
        let bitfield = Bitfield::from_bytes_length(
            &[
                0b01010101,
                0b10101010
            ],
            12
        );

        let data = vec![
            false,
            true,
            false,
            true,
            false,
            true,
            false,
            true,
            true,
            false,
            true,
            false
        ];

        assert_eq!(bitfield.iter().collect::<Vec<_>>(), data);
    }
}
