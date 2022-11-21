use std::{collections::HashMap, convert::TryFrom};

use lazy_static::lazy_static;
use thiserror::Error;

#[derive(Debug, PartialEq)]
pub struct Version {
	pub major: u8,
	pub minor: u8,
	pub patch: Option<u8>,
	pub build: Option<u8>
}

#[derive(Debug, PartialEq)]
pub enum Style {
	/// * "ABCD" => "A.B.C.D"
	Azureus,
	/// * "ABCD" => "AB.CD"
	TwoMajTwoMin,
	/// * "?ABC" => "A.BC"
	SkipFirstOneMajTwoMin,
	/// * "000A" => "0.A"
	/// * "00AB" => "0.AB"
	/// * "ABC{'Z' | 'X'}" => "A.BC+"
	/// * "ABC?" => "A.BC"
	Transmission,
	/// * "ABC?" => "A.B.C" (where A,B,C are in base 16)
	TripleHex
}

#[derive(Debug, PartialEq)]
pub struct PeerId {
	pub name: Option<String>,
	pub name_short: String,
	pub version: Version,
	random: Vec<u8>
}

#[derive(Error, Debug)]
pub enum ParseError {
	#[error("The Peer ID is of an unknown format")]
	UnknownFormat,
	#[error("The Peer ID could not be parsed because the client '{0}' is unknown")]
	UnknownClient(String),
	#[error("The Peer ID is invalid")]
	Invalid
}

pub type Result<T> = std::result::Result<T, ParseError>;

// See: https://github.com/webtorrent/bittorrent-peerid/blob/c596e92f1a9092f4d30c3ad08769d640e9fb3c6b/index.js#L188

lazy_static! {
	static ref NAMES: HashMap<&'static [u8; 2], (&'static str, Style)> = {
		let mut names = HashMap::new();
		names.insert(b"AZ", ("Azureus", Style::Azureus));
		names.insert(b"DE", ("Deluge", Style::TripleHex));
		//names.insert(b"LT", "libtorrent (Rasterbar)", Style::VER_AZ_THREE_ALPHANUMERIC_DIGITS);
		//names.insert(b"lt", "libTorrent (Rakshasa)", Style::VER_AZ_THREE_ALPHANUMERIC_DIGITS);
		//names.insert(b"LW", "LimeWire", Style::VER_NONE); // The "0001" bytes found after the LW commonly refers to the version of the BT protocol implemented. Documented here: http://www.limewire.org/wiki/index.php?title=BitTorrentRevision
		names.insert(b"PT", ("Popcorn Time", Style::Azureus));
		//names.insert(b"qB", "qBittorrent", Style::VER_AZ_DELUGE);
		//names.insert(b"TR", "Transmission", Style::VER_AZ_TRANSMISSION_STYLE);
		//names.insert(b"UE", "\u{00B5}Torrent Embedded", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
		//names.insert(b"UT", "\u{00B5}Torrent", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
		//names.insert(b"UM", "\u{00B5}Torrent Mac", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
		//names.insert(b"UW", "\u{00B5}Torrent Web", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
		//names.insert(b"WD", "WebTorrent Desktop", Style::VER_AZ_WEBTORRENT_STYLE); // Go Webtorrent!! :)
		//names.insert(b"WW", "WebTorrent", Style::VER_AZ_WEBTORRENT_STYLE);
		names
	};
}

impl TryFrom<[u8; 20]> for PeerId {
	type Error = ParseError;

	fn try_from(value: [u8; 20]) -> Result<Self> {
		fn ctoi(c: u8) -> Result<u8> {
			Ok((c as char).to_digit(10).ok_or(ParseError::Invalid)? as u8)
		}

		fn ctoix(c: u8) -> Result<u8> {
			Ok((c as char).to_digit(16).ok_or(ParseError::Invalid)? as u8)
		}

		// Azureus-style uses the following encoding: '-', two characters for client id, four ascii digits for version number, '-', followed by random numbers.

		if value[0] == b'-'
			&& value[3..7].iter().map(|&x| x as char).all(char::is_numeric)
			&& value[7] == b'-'
		{
			// Azureus-style
			let name_short: [u8; 2] = value[1..3].try_into().unwrap();
			let name = NAMES
				.get(&name_short)
				.map(|v| v.0.to_string())
				.unwrap_or_else(|| String::from_utf8(name_short.to_vec()).unwrap());

			/*
			let digits = value[3..7].iter()
				.map(|&x|
					(x as char).to_digit(10).map(|x| x as u8)
				)
				.collect::<Option<Vec<u8>>>()
				.ok_or(ParseError::NumberParseError)?;
			*/

			let digits = &value[3..7];

			let version = match NAMES
				.get(&name_short)
				.ok_or_else(|| {
					ParseError::UnknownClient(String::from_utf8(name_short.to_vec()).unwrap())
				})?
				.1
			{
				Style::Azureus => Version {
					major: ctoi(digits[0])?,
					minor: ctoi(digits[1])?,
					patch: Some(ctoi(digits[2])?),
					build: Some(ctoi(digits[3])?)
				},

				Style::SkipFirstOneMajTwoMin => Version {
					major: ctoi(digits[1])?,
					minor: ctoi(digits[2])? * 10 + ctoi(digits[3])?,
					patch: None,
					build: None
				},
				Style::Transmission =>
					if digits[0] == b'0' && digits[1] == b'0' {
						Version {
							major: 0,
							minor: ctoi(digits[2]).unwrap_or(0) * 10 + ctoi(digits[3])?,
							patch: None,
							build: None
						}
					} else {
						Version {
							major: ctoi(digits[0])?,
							minor: ctoi(digits[1])? * 10 + ctoi(digits[2])?,
							patch: None,
							build: None
						}
					},
				Style::TripleHex => Version {
					major: ctoix(digits[0])?,
					minor: ctoix(digits[1])?,
					patch: Some(ctoix(digits[2])?),
					build: None
				},
				Style::TwoMajTwoMin => Version {
					major: ctoi(digits[0])? * 10 + ctoi(digits[1])?,
					minor: ctoi(digits[2])? * 10 + ctoi(digits[3])?,
					patch: None,
					build: None
				}
			};

			let random = value[8..20].to_vec();

			Ok(PeerId {
				name: Some(name),
				name_short: String::from_utf8(name_short.to_vec()).unwrap(),
				version,
				random
			})
		} else {
			Err(ParseError::UnknownFormat)
		}

		// Shadow's style uses the following encoding: one ascii alphanumeric for client identification, up to five characters for version number (padded with '-' if less than five), followed by three characters (commonly '---', but not always the case), followed by random characters. Each character in the version string represents a number from 0 to 63. '0'=0, ..., '9'=9, 'A'=10, ..., 'Z'=35, 'a'=36, ..., 'z'=61, '.'=62, '-'=63.

		// Err(ParseError)
	}
}

#[cfg(test)]
mod tests {
	use crate::protocol::peer_id::Version;

	use super::PeerId;

	#[test]
	fn test_azureus() {
		let ver: [u8; 20] = *b"-AZ2060-000000000000";
		assert_eq!(
			PeerId::try_from(ver).unwrap(),
			PeerId {
				name: Some("Azureus".to_string()),
				name_short: "AZ".to_string(),
				version: Version {
					major: 2,
					minor: 0,
					patch: Some(6),
					build: Some(0)
				},
				random: vec![b'0'; 12]
			}
		);
	}
}
