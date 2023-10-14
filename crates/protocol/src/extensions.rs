use common::util;
use paste::paste;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NegotiationProtocol {
	/// LibTorrent Extension Protocol
	LTEP,
	/// Azureus Extended Messaging
	AZMP,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ExtensionNegotiation {
	force: bool,
	protocol: NegotiationProtocol,
}

/// | Bit   | Use                                    |
/// | ----- | -------------------------------------- |
/// | 0     | Azureus Extended Messaging             |
/// | 0-15  | BitComet Extension protocol            |
/// | 20    | BitTorrent Location-aware Protocol 1.0 |
/// | 43    | Extension protocol                     |
/// | 46-47 | Extension Negotiation Protocol         |
/// | 60    | NAT Traversal                          |
/// | 61    | Fast Peers                             |
/// | 62    | XBT Peer Exchange                      |
/// | 63    | XBT Metadata Exchange                  |
/// | 63    | DHT                                    |
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Extensions(pub [u8; 8]);

macro_rules! gen_accessors {
	($name:ident, $i: literal, $long_name:literal) => {
		paste! {
			#[doc = concat!("Get the value of the bit for ", $long_name, "\n\nReserved bit: ", $i)]
			pub fn [<get_ $name>](&self) -> bool {
				::common::util::get_bit(&self.0, $i)
			}

			#[doc = concat!("Set the value of the bit for ", $long_name, "\n\nReserved bit: ", $i)]
			pub fn [<set_ $name>](&mut self, value: bool) {
				::common::util::set_bit(&mut self.0, $i, value);
			}
		}
	};
}

impl Extensions {
	gen_accessors!(azmp, 0, "Azureus Extended Messaging Protocol");
	gen_accessors!(blap, 20, "BitTorrent Location-aware Protocol 1.0");
	gen_accessors!(extension_protocol, 43, "Extension protocol");
	gen_accessors!(nat_traversal, 60, "NAT Traversal");
	gen_accessors!(fast_peers, 61, "Fast Peers");
	gen_accessors!(xbt_peer_exchange, 62, "XBT Peer Exchange");
	gen_accessors!(xbt_metadata_exchange, 63, "XBT Metadata Exchange");
	gen_accessors!(dht, 63, "Distributed Hash Table");

	/// Get the value of the bits for BitComet Extension Protocol
	///
	/// Reserved bits: 0-15
	pub fn get_bcep(&self) -> bool {
		self.0[0] == b'e' && self.0[1] == b'x'
	}

	/// Set BitComet Extension Protocol
	///
	/// Reserved bits: 0-15
	pub fn set_bcep(&mut self, value: bool) {
		self.0[0] = if value { b'e' } else { 0 };
		self.0[1] = if value { b'x' } else { 0 };
	}

	/// Get the Extension Negotiation
	///
	/// Only valid if both peers support both AZMP and LTEP
	pub fn get_extension_negotiation(&self) -> ExtensionNegotiation {
		let force = util::get_bit(&self.0, 46) == util::get_bit(&self.0, 47);
		let protocol = if util::get_bit(&self.0, 46) {
			NegotiationProtocol::LTEP
		} else {
			NegotiationProtocol::AZMP
		};

		ExtensionNegotiation { force, protocol }
	}

	/// Set the Extension Negotiation
	///
	/// Only valid if both peers support both AZMP and LTEP
	pub fn set_extension_negotiation(&mut self, extension_negotiation: ExtensionNegotiation) {
		let b = extension_negotiation.protocol == NegotiationProtocol::LTEP;

		util::set_bit(&mut self.0, 46, b);
		util::set_bit(&mut self.0, 47, extension_negotiation.force == b);
	}
}

impl std::ops::BitAnd for Extensions {
	type Output = Extensions;

	fn bitand(self, rhs: Self) -> Self::Output {
		Extensions(std::array::from_fn(|i| self.0[i] & rhs.0[i]))
	}
}

impl std::ops::BitOr for Extensions {
	type Output = Extensions;

	fn bitor(self, rhs: Self) -> Self::Output {
		Extensions(std::array::from_fn(|i| self.0[i] | rhs.0[i]))
	}
}
