use std::convert::TryFrom;

pub struct SemVer {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    pub build: u8
}

// TODO: enum PeerId { Shadow(...), Azureus(...) }

pub struct PeerId {
    pub name: Option<String>,
    pub name_short: String,
    pub version: SemVer
    // TODO: The random string
}

#[derive(Debug)]
pub struct ParseError;

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ParseError")
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		return None;
	}
}


pub type Result<T> = std::result::Result<T, ParseError>;

// See: https://github.com/webtorrent/bittorrent-peerid/blob/c596e92f1a9092f4d30c3ad08769d640e9fb3c6b/index.js#L188

/*
names.insert(b"A~", "Ares", Style::VER_AZ_THREE_DIGITS);
names.insert(b"AG", "Ares", Style::VER_AZ_THREE_DIGITS);
names.insert(b"AN", "Ares", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"AR", "Ares", Style::AZ_FOUR_DIGITS);// Ares is more likely than ArcticTorrent
names.insert(b"AV", "Avicora", Style::AZ_FOUR_DIGITS);
names.insert(b"AX", "BitPump", Style::VER_AZ_TWO_MAJ_TWO_MIN);
names.insert(b"AT", "Artemis", Style::AZ_FOUR_DIGITS);
names.insert(b"AZ", "Vuze", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"BB", "BitBuddy', '1.234", Style::AZ_FOUR_DIGITS);
names.insert(b"BC", "BitComet", Style::VER_AZ_SKIP_FIRST_ONE_MAJ_TWO_MIN);
names.insert(b"BE", "BitTorrent SDK", Style::AZ_FOUR_DIGITS);
names.insert(b"BF", "BitFlu", Style::VER_NONE);
names.insert(b"BG", "BTG", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"bk", "BitKitten (libtorrent)", Style::AZ_FOUR_DIGITS);
names.insert(b"BR", "BitRocket', '1.2(34)", Style::AZ_FOUR_DIGITS);
names.insert(b"BS", "BTSlave", Style::AZ_FOUR_DIGITS);
names.insert(b"BT", "BitTorrent", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
names.insert(b"BW", "BitWombat", Style::AZ_FOUR_DIGITS);
names.insert(b"BX", "BittorrentX", Style::AZ_FOUR_DIGITS);
names.insert(b"CB", "Shareaza Plus", Style::AZ_FOUR_DIGITS);
names.insert(b"CD", "Enhanced CTorrent", Style::VER_AZ_TWO_MAJ_TWO_MIN);
names.insert(b"CT", "CTorrent', '1.2.34", Style::AZ_FOUR_DIGITS);
names.insert(b"DP", "Propogate Data Client", Style::AZ_FOUR_DIGITS);
names.insert(b"DE", "Deluge", Style::VER_AZ_DELUGE);
names.insert(b"EB", "EBit", Style::AZ_FOUR_DIGITS);
names.insert(b"ES", "Electric Sheep", Style::VER_AZ_THREE_DIGITS);
names.insert(b"FC", "FileCroc", Style::AZ_FOUR_DIGITS);
names.insert(b"FG", "FlashGet", Style::VER_AZ_SKIP_FIRST_ONE_MAJ_TWO_MIN);
names.insert(b"FX", "Freebox BitTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"FT", "FoxTorrent/RedSwoosh", Style::AZ_FOUR_DIGITS);
names.insert(b"GR", "GetRight', '1.2", Style::AZ_FOUR_DIGITS);
names.insert(b"GS", "GSTorrent", Style::AZ_FOUR_DIGITS); // TODO: Format is v"abcd"
names.insert(b"HL", "Halite", Style::VER_AZ_THREE_DIGITS);
names.insert(b"HN", "Hydranode", Style::AZ_FOUR_DIGITS);
names.insert(b"KG", "KGet", Style::AZ_FOUR_DIGITS);
names.insert(b"KT", "KTorrent", Style::VER_AZ_KTORRENT_STYLE);
names.insert(b"LC", "LeechCraft", Style::AZ_FOUR_DIGITS);
names.insert(b"LH", "LH-ABC", Style::AZ_FOUR_DIGITS);
names.insert(b"LK", "linkage", Style::VER_AZ_THREE_DIGITS);
names.insert(b"LP", "Lphant", Style::VER_AZ_TWO_MAJ_TWO_MIN);
names.insert(b"LT", "libtorrent (Rasterbar)", Style::VER_AZ_THREE_ALPHANUMERIC_DIGITS);
names.insert(b"lt", "libTorrent (Rakshasa)", Style::VER_AZ_THREE_ALPHANUMERIC_DIGITS);
names.insert(b"LW", "LimeWire", Style::VER_NONE); // The "0001" bytes found after the LW commonly refers to the version of the BT protocol implemented. Documented here: http://www.limewire.org/wiki/index.php?title=BitTorrentRevision
names.insert(b"MO", "MonoTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"MP", "MooPolice", Style::VER_AZ_THREE_DIGITS);
names.insert(b"MR", "Miro", Style::AZ_FOUR_DIGITS);
names.insert(b"MT", "MoonlightTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"NE", "BT Next Evolution", Style::VER_AZ_THREE_DIGITS);
names.insert(b"NX", "Net Transport", Style::AZ_FOUR_DIGITS);
names.insert(b"OS", "OneSwarm", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"OT", "OmegaTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"PC", "CacheLogic', '12.3-4", Style::AZ_FOUR_DIGITS);
names.insert(b"PT", "Popcorn Time", Style::AZ_FOUR_DIGITS);
names.insert(b"PD", "Pando", Style::AZ_FOUR_DIGITS);
names.insert(b"PE", "PeerProject", Style::AZ_FOUR_DIGITS);
names.insert(b"pX", "pHoeniX", Style::AZ_FOUR_DIGITS);
names.insert(b"qB", "qBittorrent", Style::VER_AZ_DELUGE);
names.insert(b"QD", "qqdownload", Style::AZ_FOUR_DIGITS);
names.insert(b"RT", "Retriever", Style::AZ_FOUR_DIGITS);
names.insert(b"RZ", "RezTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"S~", "Shareaza alpha/beta", Style::AZ_FOUR_DIGITS);
names.insert(b"SB", "SwiftBit", Style::AZ_FOUR_DIGITS);
names.insert(b"SD", "\u8FC5\u96F7\u5728\u7EBF (Xunlei)", Style::AZ_FOUR_DIGITS); // Apparently, the English name of the client is "Thunderbolt".
names.insert(b"SG", "GS Torrent", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"SN", "ShareNET", Style::AZ_FOUR_DIGITS);
names.insert(b"SP", "BitSpirit", Style::VER_AZ_THREE_DIGITS); // >= 3.6
names.insert(b"SS", "SwarmScope", Style::AZ_FOUR_DIGITS);
names.insert(b"ST", "SymTorrent', '2.34", Style::AZ_FOUR_DIGITS);
names.insert(b"st", "SharkTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"SZ", "Shareaza", Style::AZ_FOUR_DIGITS);
names.insert(b"TG", "Torrent GO", Style::AZ_FOUR_DIGITS);
names.insert(b"TN", "Torrent.NET", Style::AZ_FOUR_DIGITS);
names.insert(b"TR", "Transmission", Style::VER_AZ_TRANSMISSION_STYLE);
names.insert(b"TS", "TorrentStorm", Style::AZ_FOUR_DIGITS);
names.insert(b"TT", "TuoTu", Style::VER_AZ_THREE_DIGITS);
names.insert(b"UL", "uLeecher!", Style::AZ_FOUR_DIGITS);
names.insert(b"UE", "\u00B5Torrent Embedded", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
names.insert(b"UT", "\u00B5Torrent", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
names.insert(b"UM", "\u00B5Torrent Mac", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
names.insert(b"UW", "\u00B5Torrent Web", Style::VER_AZ_THREE_DIGITS_PLUS_MNEMONIC);
names.insert(b"WD", "WebTorrent Desktop", Style::VER_AZ_WEBTORRENT_STYLE); // Go Webtorrent!! :)
names.insert(b"WT", "Bitlet", Style::AZ_FOUR_DIGITS);
names.insert(b"WW", "WebTorrent", Style::VER_AZ_WEBTORRENT_STYLE)// Go Webtorrent!! :);
names.insert(b"WY", "FireTorrent", Style::AZ_FOUR_DIGITS);// formerly Wyzo.
names.insert(b"VG", "\u54c7\u560E (Vagaa)", Style::VER_AZ_FOUR_DIGITS);
names.insert(b"XL", "\u8FC5\u96F7\u5728\u7EBF (Xunlei)", Style::AZ_FOUR_DIGITS); // Apparently, the English name of the client is "Thunderbolt".
names.insert(b"XT", "XanTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"XF", "Xfplay", Style::VER_AZ_TRANSMISSION_STYLE);
names.insert(b"XX", "XTorrent', '1.2.34", Style::AZ_FOUR_DIGITS);
names.insert(b"XC", "XTorrent', '1.2.34", Style::AZ_FOUR_DIGITS);
names.insert(b"ZT", "ZipTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"7T", "aTorrent", Style::AZ_FOUR_DIGITS);
names.insert(b"ZO", "Zona", Style::VER_AZ_FOUR_DIGITS);
*/

impl TryFrom<[u8; 20]> for PeerId {
    type Error = ParseError;

    fn try_from(value: [u8; 20]) -> Result<Self> {
        // Azureus-style uses the following encoding: '-', two characters for client id, four ascii digits for version number, '-', followed by random numbers. 

        if value[0] == b'-'
            && value[3..7]
                .iter()
                .map(|&x| x as char)
                .all(char::is_numeric)
            && value[7] == b'-' {
            // Azureus-style
            let name_short = &value[1..2];

            let digits = value[3..7].iter()
                .map(|&x|
                    (x as char).to_digit(10).map(|x| x as u8)
                )
                .collect::<Option<Vec<u8>>>()
                .ok_or(ParseError)?;

            let version = SemVer {
                major: digits[0],
                minor: digits[1],
                patch: digits[2],
                build: digits[3],
            };
        }
        

        // Shadow's style uses the following encoding: one ascii alphanumeric for client identification, up to five characters for version number (padded with '-' if less than five), followed by three characters (commonly '---', but not always the case), followed by random characters. Each character in the version string represents a number from 0 to 63. '0'=0, ..., '9'=9, 'A'=10, ..., 'Z'=35, 'a'=36, ..., 'z'=61, '.'=62, '-'=63. 
        
        // Err(ParseError)

        todo!()
    }
}
