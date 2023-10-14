use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
#[error("Bitfield error: {msg}")]
pub struct Error {
	msg: String,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bitfield {
	data: Vec<u8>,
	length: usize,
}

impl Bitfield {
	pub fn new(length: usize) -> Bitfield {
		Bitfield {
			data: vec![0; length.div_ceil(8)],
			length,
		}
	}

	#[deprecated = "Use from_bytes_length instead"]
	pub fn from_bytes(data: &[u8]) -> Bitfield {
		Bitfield {
			data: data.to_vec(),
			length: data.len() * 8,
		}
	}

	/// Converts the given bytes into a bitfield.
	///
	/// # Errors
	/// Returns an error if
	/// * the remaining bits are not all set to 0.
	/// * there are not enough bytes to store `length` bits (`data.len() * 8 < length`)
	/// * there are 8 or more remaining bits
	pub fn from_bytes_length(data: &[u8], length: usize) -> Result<Bitfield> {
		if data.len() * 8 < length {
			return Err(Error {
				msg: format!(
					"data.len() * 8 >= length failed. data.len() = {}, length = {}",
					data.len(),
					length
				),
			});
		}

		let remaining_bits = data.len() * 8 - length;

		if remaining_bits >= 8 {
			return Err(Error {
				msg: format!("remaining_bits = {remaining_bits}"),
			});
		}

		let bitfield = Bitfield {
			data: data.to_vec(),
			length,
		};

		for i in length..(data.len() * 8) {
			if bitfield.get(i) {
				return Err(Error {
					msg: format!("bitfield[{i}] != 0"),
				});
			}
		}

		Ok(bitfield)
	}

	/// Get the internal vector used to represent the bitfield
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
			index: 0,
		}
	}

	pub fn from_bools(bools: &[bool]) -> Bitfield {
		let mut bitfield = Bitfield::new(bools.len());

		for (i, bool) in bools.iter().enumerate() {
			bitfield.set(i, *bool);
		}

		bitfield
	}

	/// Get the number of bits in the bitfield
	pub fn len(&self) -> usize {
		self.length
	}

	/// Get the size of the bitfield in bytes, in other words `length.div_ceil(8)`
	pub fn size(&self) -> usize {
		self.data.len()
	}

	/// Returns true if the bitfield is empty
	pub fn is_empty(&self) -> bool {
		self.length == 0
	}
}

pub struct BitfieldIterator<'a> {
	bitfield: &'a Bitfield,
	index: usize,
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
			index: 0,
		}
	}
}

impl std::ops::BitOr for Bitfield {
	type Output = Bitfield;

	fn bitor(self, rhs: Self) -> Self::Output {
		assert_eq!(self.len(), rhs.len());

		Bitfield::from_bools(
			&self
				.iter()
				.zip(rhs.iter())
				.map(|(a, b)| a | b)
				.collect::<Vec<_>>(),
		)
	}
}

impl std::ops::BitOrAssign for &mut Bitfield {
	fn bitor_assign(&mut self, rhs: Self) {
		assert_eq!(self.len(), rhs.len());

		for (i, value) in rhs.iter().enumerate() {
			self.set(i, value);
		}
	}
}

impl From<Bitfield> for Vec<u8> {
	fn from(value: Bitfield) -> Self {
		value.data
	}
}

#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;

	#[test]
	fn test_len() {
		assert_eq!(Bitfield::new(42).len(), 42);
		assert_eq!(
			Bitfield::from_bytes_length(&[0b01010101, 0b10101010], 16)
				.unwrap()
				.len(),
			16
		);
		assert_eq!(
			Bitfield::from_bytes_length(&[0b01010101, 0b10100000], 12)
				.unwrap()
				.len(),
			12
		);
	}

	#[test]
	fn test_get() {
		let bitfield = Bitfield::from_bytes_length(&[0b01010101, 0b10100000], 12).unwrap();

		for i in 0..12 {
			let expected = i % 2 == if i < 8 { 1 } else { 0 };
			assert_eq!(bitfield.get(i), expected);
		}
	}

	#[test]
	fn test_iterator() {
		let bitfield = Bitfield::from_bytes_length(&[0b01010101, 0b10100000], 12).unwrap();

		let data = vec![
			false, true, false, true, false, true, false, true, true, false, true, false,
		];

		assert_eq!(bitfield.iter().collect::<Vec<_>>(), data);
	}
}
