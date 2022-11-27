use lazy_static::lazy_static;
use thiserror::Error;

use crate::core::util;

lazy_static! {
	/// List of parseable clients
	static ref CLIENTS: Vec<Client> = {
		serde_json::from_str::<serde_json::Value>(include_str!("clients.json"))
			.unwrap()
			.as_array()
			.unwrap()
			.iter()
			.map(|object|
				object
					.as_object()
					.unwrap()
			)
			.filter(|object| object.get("style").map(|str| str.as_str().unwrap()) != Some("VER_NONE"))
			.map(|object|
				Client {
					short: object.get("short").unwrap().as_str().unwrap().as_bytes().to_vec(),
					long: object.get("long").unwrap().as_str().unwrap().to_string(),
					style: object.get("style").map(|str| match str.as_str().unwrap() {
						"VER_AZ_THREE_DIGITS" => Style::Three,
						"VER_AZ_THREE_DIGITS_PLUS_MNEMONIC" => Style::Three, // TODO: Support mnemonics as well
						"VER_AZ_FOUR_DIGITS" => Style::OneOneOneOne,
						"VER_AZ_TWO_MAJ_TWO_MIN" => Style::TwoTwo,
						"VER_AZ_ONE_THREE" => Style::OneThree,
						"VER_AZ_MAJ_MIN_TWO_SKIP" => Style::OneOne,
						"VER_AZ_SKIP_FIRST_ONE_MAJ_TWO_MIN" => Style::SkipOneTwo,
						style => Style::Unknown(style.to_string())
					})
					.unwrap_or(Style::OneOneOneOne)
				}
			)
			.collect::<Vec<_>>()
	};
}

#[derive(Debug, PartialEq, Clone)]
pub enum Style {
	/// * "ABCD" => "A.B.C.D"
	OneOneOneOne,
	/// * "ABCD" => "AB.CD"
	TwoTwo,
	/// * "ABC?" => "A.B.C"
	Three,
	/// * "?ABC" => "A.BC"
	SkipOneTwo,
	/// * "000A" => "0.A"
	/// * "00AB" => "0.AB"
	/// * "ABC{'Z' | 'X'}" => "A.BC+"
	/// * "ABC?" => "A.BC"
	Transmission,
	/// * "ABC?" => "A.B.C" (where A,B,C are in base 16)
	ThreeHex,
	/// * "ABCD" => "A.BCD"
	OneThree,
	/// * "AB??" => "A.B"
	OneOne,
	/// Unknown style
	Unknown(String)
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Version {
	pub major: u8,
	pub minor: u8,
	pub patch: Option<u8>,
	pub build: Option<u8>
}

impl Version {
	pub fn new(major: u8, minor: u8, patch: Option<u8>, build: Option<u8>) -> Version {
		Version {
			major,
			minor,
			patch,
			build
		}
	}

	pub fn parse(digits: &[u8], style: Style) -> Result<Version> {
		fn ctoi(c: u8) -> Result<u8> {
			Ok((c as char).to_digit(10).ok_or(ParseError::Invalid)? as u8)
		}

		fn ctoix(c: u8) -> Result<u8> {
			Ok((c as char).to_digit(16).ok_or(ParseError::Invalid)? as u8)
		}

		Ok(match style {
			Style::OneOneOneOne => Version {
				major: ctoi(digits[0])?,
				minor: ctoi(digits[1])?,
				patch: Some(ctoi(digits[2])?),
				build: Some(ctoi(digits[3])?)
			},

			Style::SkipOneTwo => Version {
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
			Style::Three => Version {
				major: ctoi(digits[0])?,
				minor: ctoi(digits[1])?,
				patch: Some(ctoi(digits[2])?),
				build: None
			},
			Style::ThreeHex => Version {
				major: ctoix(digits[0])?,
				minor: ctoix(digits[1])?,
				patch: Some(ctoix(digits[2])?),
				build: None
			},
			Style::OneThree => Version {
				major: ctoi(digits[0])?,
				minor: ctoi(digits[1])? * 100 + ctoi(digits[2])? * 10 + ctoi(digits[3])?,
				patch: None,
				build: None
			},
			Style::OneOne => Version {
				major: ctoi(digits[0])?,
				minor: ctoi(digits[1])?,
				patch: None,
				build: None
			},
			Style::TwoTwo => Version {
				major: ctoi(digits[0])? * 10 + ctoi(digits[1])?,
				minor: ctoi(digits[2])? * 10 + ctoi(digits[3])?,
				patch: None,
				build: None
			},
			Style::Unknown(_) => return Err(ParseError::UnknownFormat)
		})
	}
}

impl std::fmt::Display for Version {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}.{}", self.major, self.minor)?;

		if let Some(patch) = self.patch {
			write!(f, ".{}", patch)?;

			if let Some(build) = self.build {
				write!(f, ".{}", build)?;
			}
		}

		Ok(())
	}
}

#[derive(Debug, PartialEq)]
pub struct PeerId {
	pub name: String,
	pub name_short: String,
	pub version: Version,
	random: Vec<u8>
}

impl PeerId {
	pub fn parse(value: &[u8]) -> Result<Self> {
		if value.len() != 20 {
			return Err(ParseError::Invalid);
		}

		// Azureus-style uses the following encoding: '-', two characters for client id, four ascii digits for version number, '-', followed by random numbers.

		if value[0] == b'-'
		//	&& value[3..7].iter().map(|&x| x as char).all(char::is_numeric)
			&& value[7] == b'-'
		{
			// Azureus-style
			let name_short = &value[1..3];
			let random = &value[8..20];
			let digits = &value[3..7];

			let client = CLIENTS
				.iter()
				.find(|client| client.short == name_short)
				.ok_or(ParseError::UnknownClient(
					String::from_utf8(name_short.to_vec()).map_err(|_| ParseError::Invalid)?
				))?;

			let version = Version::parse(digits, client.style.clone())?;

			return Ok(PeerId {
				name: client.long.clone(),
				name_short: String::from_utf8(name_short.to_vec()).unwrap(),
				version,
				random: random.to_vec()
			});
		}

		// Shadow's style uses the following encoding: one ascii alphanumeric for client identification, up to five characters for version number (padded with '-' if less than five), followed by three characters (commonly '---', but not always the case), followed by random characters. Each character in the version string represents a number from 0 to 63. '0'=0, ..., '9'=9, 'A'=10, ..., 'Z'=35, 'a'=36, ..., 'z'=61, '.'=62, '-'=63.

		Err(ParseError::UnknownFormat)
	}

	pub fn parse_str(value: &str) -> Result<Self> {
		Self::parse(value.as_bytes())
	}

	pub fn parse_hex(value: &str) -> Result<Self> {
		Self::parse(&util::parse_hex(value).map_err(|_| ParseError::Invalid)?)
	}
}

impl std::fmt::Display for PeerId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{} {}", self.name, self.version)
	}
}

#[derive(Error, Debug, PartialEq)]
pub enum ParseError {
	#[error("The Peer ID is of an unknown format")]
	UnknownFormat,
	#[error("The Peer ID could not be parsed because the client '{0}' is unknown")]
	UnknownClient(String),
	#[error("The Peer ID is invalid")]
	Invalid
}

pub type Result<T> = std::result::Result<T, ParseError>;

struct Client {
	pub short: Vec<u8>,
	pub long: String,
	pub style: Style
}

// See: https://github.com/webtorrent/bittorrent-peerid/blob/c596e92f1a9092f4d30c3ad08769d640e9fb3c6b/index.js#L188

/// Grasciously stolen from bittorrent-peerid
/// https://github.com/webtorrent/bittorrent-peerid/blob/master/test/basic.js
#[cfg(test)]
mod tests {
	use crate::{core::util, protocol::peer_id::Version};

	use super::PeerId;

	#[test]
	fn test_azureus() {
		let ver: [u8; 20] = *b"-AZ2060-000000000000";
		assert_eq!(
			PeerId::parse(&ver).unwrap(),
			PeerId {
				name: "Vuze".to_string(),
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

	#[test]
	fn test_one_one_one_one() {
		assert_eq!(
			PeerId::parse(b"-AT2520-vEEt0wO6v0cr"),
			Ok(PeerId {
				name: "Artemis".to_string(),
				name_short: "AT".to_string(),
				version: Version::new(2, 5, Some(2), Some(0)),
				random: b"vEEt0wO6v0cr".to_vec()
			})
		);

		assert_eq!(
			PeerId::parse(b"-AZ2200-6wfG2wk6wWLc"),
			Ok(PeerId {
				name: "Vuze".to_string(),
				name_short: "AZ".to_string(),
				version: Version::new(2, 2, Some(0), Some(0)),
				random: b"6wfG2wk6wWLc".to_vec()
			})
		);

		assert_eq!(
			PeerId::parse_hex("2D535A323133322D000000000000000000000000"),
			Ok(PeerId {
				name: "Shareaza".to_string(),
				name_short: "SZ".to_string(),
				version: Version::new(2, 1, Some(3), Some(2)),
				random: util::parse_hex("000000000000000000000000").unwrap()
			})
		);

		assert_eq!(
			PeerId::parse_hex("2D5647323634342D4FD62CDA69E235717E3BB94B"),
			Ok(PeerId {
				name: "\u{54c7}\u{560E} (Vagaa)".to_string(),
				name_short: "VG".to_string(),
				version: Version::new(2, 6, Some(4), Some(4)),
				random: util::parse_hex("4FD62CDA69E235717E3BB94B").unwrap()
			})
		);

		assert_eq!(
			PeerId::parse(b"-WY0300-6huHF5Pr7Vde"),
			Ok(PeerId {
				name: "FireTorrent".to_string(),
				name_short: "WY".to_string(),
				version: Version::new(0, 3, Some(0), Some(0)),
				random: b"6huHF5Pr7Vde".to_vec()
			})
		);

		assert_eq!(
			PeerId::parse(b"-KG2450-BDEw8OM14Hk6"),
			Ok(PeerId {
				name: "KGet".to_string(),
				name_short: "KG".to_string(),
				version: Version::new(2, 4, Some(5), Some(0)),
				random: b"BDEw8OM14Hk6".to_vec()
			})
		);
	}

	#[test]
	fn test_two_two() {
		assert_eq!(
			PeerId::parse_hex("2D4C50303330322D003833363536393537373030"),
			Ok(PeerId {
				name: "Lphant".to_string(),
				name_short: "LP".to_string(),
				version: Version::new(3, 2, None, None),
				random: util::parse_hex("003833363536393537373030").unwrap()
			})
		);
	}

	#[test]
	fn test_three() {
		assert_eq!(
			PeerId::parse(b"-HL0290-xUO*9ugvENUE"),
			Ok(PeerId {
				name: "Halite".to_string(),
				name_short: "HL".to_string(),
				version: Version::new(0, 2, Some(9), None),
				random: b"xUO*9ugvENUE".to_vec()
			})
		);

		assert_eq!(
			PeerId::parse(b"-LK0140-ATIV~nbEQAMr"),
			Ok(PeerId {
				name: "linkage".to_string(),
				name_short: "LK".to_string(),
				version: Version::new(0, 1, Some(4), None),
				random: b"ATIV~nbEQAMr".to_vec()
			})
		);

		assert_eq!(
			PeerId::parse(b"-TT210w-dq!nWf~Qcext"),
			Ok(PeerId {
				name: "TuoTu".to_string(),
				name_short: "TT".to_string(),
				version: Version::new(2, 1, Some(0), None),
				random: b"dq!nWf~Qcext".to_vec()
			})
		);

		// TODO: Three digits plus mnemonic (Alpha, Beta)
		assert_eq!(
			PeerId::parse_hex("2D5554313730422D928446441DB0A094A01C01E5"),
			Ok(PeerId {
				name: "\u{00B5}Torrent".to_string(),
				name_short: "UT".to_string(),
				version: Version::new(1, 7, Some(0), None),
				random: util::parse_hex("928446441DB0A094A01C01E5").unwrap()
			})
		);
	}

	#[test]
	fn skip_one_two() {
		assert_eq!(
			PeerId::parse(b"-ST0117-01234567890!"),
			Ok(PeerId {
				name: "SymTorrent".to_string(),
				name_short: "ST".to_string(),
				version: Version::new(1, 17, None, None),
				random: b"01234567890!".to_vec()
			})
		);
	}

	#[test]
	fn test_special() {
		// Style::Three without dash
		/*
		assert_eq!(PeerId::parse(b"-NE1090002IKyMn4g7Ko"), Ok(PeerId {
			name: "BT Next Evolution".to_string(),
			name_short: "NE".to_string(),
			version: Version::new(1, 0, Some(9), None),
			random: b"02IKyMn4g7Ko".to_vec()
		}));
		*/

		// Style::SkipOneTwo without dash
		/*
		assert_eq!(PeerId::parse_hex("2D46473031383075F80057821359D64BB3DFD265"), Ok(PeerId {
			name: "FlashGet".to_string(),
			name_short: "FG".to_string(),
			version: Version::new(1, 80, None, None),
			random: util::parse_hex("75F80057821359D64BB3DFD265").unwrap()
		}));
		*/

		// Style::BitComet
		// 6578626300387A4463102D6E9AD6723B339F35A9

		// TODO: A.B.C-D
		/*
		assert_eq!(PeerId::try_from(b"-PC251Q-6huHF5Pr7Vde"), Ok(PeerId {
			name: "CacheLogic".to_string(),
			name_short: "PC".to_string(),
			version: todo!(),
			random: todo!()
		}));
		*/
	}

	#[test]
	fn test_one_one() {
		assert_eq!(
			PeerId::parse(b"-GR6300-13s3iFKmbArc"),
			Ok(PeerId {
				name: "GetRight".to_string(),
				name_short: "GR".to_string(),
				version: Version::new(6, 3, None, None),
				random: b"13s3iFKmbArc".to_vec()
			})
		);
	}

	// TODO: Support more peer ids
	/*
	assert_eq!(PeerId::parse(b"-AG2053-Em6o1EmvwLtD"), Ok(PeerId {
		name: "Ares".to_string(),
		name_short: "AG".to_string(),
		version: Version::new(2, 0, Some(5), None), // Are we sure it's not 2.0.5.3?
		random: b"Em6o1EmvwLtD".to_vec()
	}));

	assert_eq!(PeerId::parse(b"-AR1670-3Ql6wM3hgtCc"), Ok(PeerId {
		name: "Ares".to_string(),
		name_short: "AR".to_string(),
		version: Version::new(1, 6, Some(7), Some(0)), // TODO: Are we sure it's not 1.6.7?
		random: b"3Ql6wM3hgtCc".to_vec()
	}));

	assert_eq!(PeerId::try_from(b"-BR0332-!XVceSn(*KIl"), Ok(PeerId {
		name: "BitRocket".to_string(),
		name_short: "BR".to_string(),
		version: todo!(),
		random: todo!()
	}));

	assert_eq!(PeerId::try_from(b"-KT11R1-693649213030"), Ok(PeerId {
		name: "KTorrent".to_string(),
		name_short: "KT".to_string(),
		version: todo!(),
		random: b"693649213030".to_vec()
	}));

	assert_eq!(PeerId::try_from(<&[u8] as TryInto<&[u8; 20]>>::try_into(&util::parse_hex("2D4B543330302D006A7139727958377731756A4B").unwrap()).unwrap()), Ok(PeerId {
		name: "KTorrent".to_string(),
		name_short: "KT".to_string(),
		version: todo!(),
		random: todo!()
	}));

	assert_eq!(PeerId::try_from(<&[u8] as TryInto<&[u8; 20]>>::try_into(&util::parse_hex("2D6C74304232302D0D739B93E6BE21FEBB557B20").unwrap()).unwrap()), Ok(PeerId {
		name: "libTorrent (Rakshasa) / rTorrent*".to_string(),
		name_short: "lt".to_string(),
		version: todo!(),
		random: util::parse_hex("739B93E6BE21FEBB557B20").unwrap()
	}));

	assert_eq!(PeerId::try_from(b"-LT0D00-eZ0PwaDDr-~v"), Ok(PeerId {
		name: "libtorrent (Rasterbar)".to_string(),
		name_short: "LT".to_string(),
		version: todo!(),
		random: b"eZ0PwaDDr-~v".to_vec()
	}));

	assert_eq!(PeerId::try_from(<&[u8] as TryInto<&[u8; 20]>>::try_into(&util::parse_hex("2D4C57303030312D31E0B3A0B46F7D4E954F4103").unwrap()).unwrap()), Ok(PeerId {
		name: "LimeWire".to_string(),
		name_short: "D4".to_string(),
		version: todo!(),
		random: todo!()
	}));

	assert_eq!(PeerId::try_from(b"-TR0006-01234567890!"), Ok(PeerId {
		name: "Transmission".to_string(),
		name_short: "TR".to_string(),
		version: todo!(),
		random: todo!()
	}));

	assert_eq!(PeerId::try_from(b"-TR072Z-zihst5yvg22f"), Ok(PeerId {
		name: "Transmission".to_string(),
		name_short: "TR".to_string(),
		version: todo!(),
		random: todo!()
	}));

	assert_eq!(PeerId::try_from(b"-TR0072-8vd6hrmp04an"), Ok(PeerId {
		name: "Transmission".to_string(),
		name_short: "TR".to_string(),
		version: todo!(),
		random: todo!()
	}));
	*/
}
