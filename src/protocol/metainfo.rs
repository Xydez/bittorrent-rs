use sha1::Digest;
use std::convert::TryFrom;
use std::io::Read;

// impl std::fmt::Display for Hash {
// 	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
// 		for b in &self.0 {
// 			write!(f, "{:x}", b)?;
// 		}

// 		Ok(())
// 	}
// }

#[derive(Debug)]
pub enum MetadataError {
    IOError(std::io::Error),
    BencodeError(serde_bencode::Error),
    InvalidMetadata,
    InvalidPieces,
    InvalidTrackerUrl,
}

impl From<std::io::Error> for MetadataError {
    fn from(error: std::io::Error) -> Self {
        MetadataError::IOError(error)
    }
}

impl From<serde_bencode::Error> for MetadataError {
    fn from(error: serde_bencode::Error) -> Self {
        MetadataError::BencodeError(error)
    }
}

pub(crate) type Result<T> = std::result::Result<T, MetadataError>;

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::IOError(error) => write!(f, "IO Error: {}", error),
            Self::BencodeError(error) => write!(f, "Bencode error: {}", error),
            Self::InvalidMetadata => write!(f, "Invalid metadata"),
            Self::InvalidPieces => write!(f, "Invalid pieces"),
            Self::InvalidTrackerUrl => write!(f, "Invalid tracker url"),
        }
    }
}

// impl std::error::Error for MetadataError {
// 	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
// 		None
// 	}
// }

#[derive(Clone)]
pub struct MetaInfo {
    pub name: String,
    pub length: usize,
    pub announce: String,
    pub info_hash: [u8; 20],
    pub pieces: Vec<[u8; 20]>,
    pub piece_size: usize,
    pub last_piece_size: usize,
    pub files: Vec<FileInfo>,
}

// struct StopErrorIter<T, I: Iterator<Item = T>> {
// 	input: I
// }

// impl<T, I: Iterator<Item = T>> StopErrorIter<T, I> {
// 	pub fn new(input: I) -> StopErrorIter<T, I> {
// 		StopErrorIter { input }
// 	}
// }

// impl<T, I: Iterator<Item = T>> Iterator for StopErrorIter<T, I> {
// 	type Item = T;

// 	fn next(&mut self) -> Option<Self::Item> {

// 	}
// }

impl TryFrom<&[u8]> for MetaInfo {
    type Error = MetadataError;

    /// Load the MetaInfo from raw bencode
    fn try_from(buf: &[u8]) -> Result<Self> {
        let metadata: raw::MetaInfo = serde_bencode::from_bytes(buf)?;

        let mut info_hash = [0u8; 20];
        let digest = sha1::Sha1::digest(&serde_bencode::to_bytes(&metadata.info)?);
        info_hash.copy_from_slice(&digest);

        let pieces = metadata
            .info
            .pieces
            .chunks_exact(20)
            .map(|chunk| <[u8; 20]>::try_from(chunk).map_err(|_| MetadataError::InvalidMetadata))
            .collect::<Result<Vec<[u8; 20]>>>()?;
        // .into_iter()
        // .map(|chunk| Hash(chunk))
        // .collect::<Vec<[u8; 20]>>();

        let files = match metadata.info.files {
            Some(files) => {
                let mut file_infos = Vec::new();

                let mut offset = 0;

                for file in files {
                    file_infos.push(FileInfo {
                        path: file.path.iter().collect(),
                        length: file.length,
                        offset,
                    });

                    offset += file.length;
                }

                file_infos
            }
            None => match metadata.info.length {
                None => return Err(MetadataError::InvalidMetadata),
                Some(length) => vec![FileInfo {
                    path: metadata.info.name.clone().into(),
                    length,
                    offset: 0,
                }],
            },
        };

        let length = files.iter().fold(0, |acc, x| acc + x.length);

        Ok(MetaInfo {
            name: metadata.info.name,
            length,
            announce: metadata.announce,
            info_hash,
            pieces,
            piece_size: metadata.info.piece_length,
            last_piece_size: if length % metadata.info.piece_length == 0 {
                metadata.info.piece_length
            } else {
                length % metadata.info.piece_length
            },
            files,
        })
    }
}

impl MetaInfo {
    /// Read a bencoded file into a MetaInfo
    pub fn load(path: &str) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        MetaInfo::try_from(buf.as_slice())
    }
}

impl std::fmt::Debug for MetaInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaInfo")
            .field("name", &self.name)
            .field("tracker", &self.announce)
            .field("info_hash", &self.info_hash)
            .field("pieces", &format!("<{} pieces>", &self.pieces.len()))
            .field("piece_length", &self.piece_size)
            .field("files", &self.files)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: std::path::PathBuf,
    pub length: usize,
    pub offset: usize,
}

mod raw {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize)]
    pub struct MetaInfo {
        pub announce: String,
        pub info: Info,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Info {
        /// A list of dictionaries each corresponding to a file (only when multiple files are being shared)
        pub files: Option<Vec<File>>,
        /// Size of the file in bytes (only when one file is being shared)
        pub length: Option<usize>,
        /// Suggested filename where the file is to be saved (if one file)/suggested directory name where the files are to be saved (if multiple files)
        pub name: String,
        /// Number of bytes per piece
        #[serde(rename = "piece length")]
        pub piece_length: usize,
        /// A hash list, i.e., a concatenation of each piece's SHA-1 hash. As SHA-1 returns a 160-bit hash, pieces will be a string whose length is a multiple of 20 bytes. If the torrent contains multiple files, the pieces are formed by concatenating the files in the order they appear in the files dictionary (i.e. all pieces in the torrent are the full piece length except for the last piece, which may be shorter)
        pub pieces: serde_bytes::ByteBuf,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct File {
        /// Size of the file in bytes.
        pub length: usize,
        /// A list of strings corresponding to subdirectory names, the last of which is the actual file name
        pub path: Vec<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse() {
        MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();
    }
}
