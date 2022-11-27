#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub struct ParseError;

impl std::fmt::Display for ParseError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Parse error")
	}
}

/// Print a slice of [`u8`] as a hexadecimal string
///
/// ```rust
/// use bittorrent::core::util::hex;
///
/// assert_eq!(hex(&[0x1a, 0x2b, 0x3c]), "1a2b3c".to_string())
/// ```
pub fn hex(bytes: &[u8]) -> String {
	let mut out = String::with_capacity(bytes.len() * 3 - 1);

	for b in bytes {
		out.push_str(&format!("{:02x}", b));
	}

	out
}

/// Parse a string of hexadecimal characters with length mod 2 into a vec of [`u8`]
///
/// ```rust
/// use bittorrent::core::util::parse_hex;
///
/// assert_eq!(parse_hex("1a2b3c"), Ok(vec![0x1a, 0x2b, 0x3c]));
/// assert!(parse_hex("ghijkl").is_err()); // Contains non-hexadecimal characters
/// assert!(parse_hex("12345").is_err()); // Length of 5 is not mod 2
/// ```
pub fn parse_hex(str: &str) -> Result<Vec<u8>, ParseError> {
	let mut nums = Vec::new();

	if str.len() % 2 != 0 {
		return Err(ParseError);
	}

	for i in (0..str.len()).step_by(2) {
		nums.push(u8::from_str_radix(&str[i..(i + 2)], 16).map_err(|_| ParseError)?);
	}

	Ok(nums)
}

pub fn group_by_key<I, V, K, F>(elements: I, mut f: F) -> std::collections::BTreeMap<K, Vec<V>>
where
	I: IntoIterator<Item = V>,
	K: Eq + std::cmp::Ord,
	F: FnMut(&V) -> K
{
	let mut map: std::collections::BTreeMap<K, Vec<V>> = std::collections::BTreeMap::new();

	for value in elements.into_iter() {
		map.entry(f(&value)).or_default().push(value);
	}

	map
}

pub fn error_chain<E: std::error::Error>(error: E) -> String {
	let mut string = format!("{}", error);

	let mut current = error.source();
	while let Some(cause) = current {
		string.push_str(&format!("\nCaused by:\n\t{}", cause));
		current = cause.source();
	}

	string
}

#[inline]
pub fn get_bit(bytes: &[u8], i: usize) -> bool {
	let n = i / 8;
	let m = 7 - i % 8;

	bytes[n] & (1 << m) != 0
}

#[inline]
pub fn set_bit(bytes: &mut [u8], i: usize, value: bool) {
	let n = i / 8;
	let m = 7 - i % 8;

	if value {
		bytes[n] |= 1 << m;
	} else {
		bytes[n] &= !(1 << m);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_hex() {
		let bytes = [0xab, 0xf1, 0x00, 0x3f];

		assert_eq!(hex(&bytes).as_str(), "abf1003f");
	}

	#[test]
	fn test_get_bit() {
		let bytes = [0b00000000, 0b01000000];

		for i in 0..(std::mem::size_of_val(&bytes) * 8) {
			assert_eq!(get_bit(&bytes, i), i == 9);
		}
	}

	#[test]
	fn test_set_bit() {
		let mut bytes = [0b00000000, 0b00000000];

		for i in 0..(std::mem::size_of_val(&bytes) * 8) {
			set_bit(&mut bytes, i, i % 2 == 0);
		}

		assert_eq!(bytes, [0b10101010, 0b10101010]);
	}
}
