use sha1::Digest;
use thiserror::Error;
use std::convert::TryFrom;
use std::io::Read;

#[derive(Error, Debug)]
pub enum MetaInfoError {
    #[error("An IO error occurred")]
    IOError(#[from] std::io::Error),
    #[error("Failed to serialize or deserialize bencode")]
    BencodeError(#[from] serde_bencode::Error),
    #[error("The meta info is invalid")]
    InvalidMetaInfo,
}

pub type Result<T> = std::result::Result<T, MetaInfoError>;

#[derive(Clone, Debug, PartialEq, Eq)]
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

impl TryFrom<&[u8]> for MetaInfo {
    type Error = MetaInfoError;

    /// Load the MetaInfo from raw bencode
    fn try_from(buf: &[u8]) -> Result<Self> {
        let metadata: raw::MetaInfo = serde_bencode::from_bytes(buf)?;

        let mut info_hash = [0u8; 20];
        let digest = sha1::Sha1::digest(serde_bencode::to_bytes(&metadata.info)?);
        info_hash.copy_from_slice(&digest);

        let pieces = metadata
            .info
            .pieces
            .chunks_exact(20)
            .map(|chunk| <[u8; 20]>::try_from(chunk).map_err(|_| MetaInfoError::InvalidMetaInfo))
            .collect::<Result<Vec<[u8; 20]>>>()?;

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
                None => return Err(MetaInfoError::InvalidMetaInfo),
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

    const SAMPLE_TORRENT: &[u8] = &[100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 51, 53, 58, 117, 100, 112, 58, 47, 47, 116, 114, 97, 99, 107, 101, 114, 46, 111, 112, 101, 110, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 46, 99, 111, 109, 58, 56, 48, 49, 51, 58, 99, 114, 101, 97, 116, 105, 111, 110, 32, 100, 97, 116, 101, 105, 49, 51, 50, 55, 48, 52, 57, 56, 50, 55, 101, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103, 116, 104, 105, 50, 48, 101, 52, 58, 110, 97, 109, 101, 49, 48, 58, 115, 97, 109, 112, 108, 101, 46, 116, 120, 116, 49, 50, 58, 112, 105, 101, 99, 101, 32, 108, 101, 110, 103, 116, 104, 105, 54, 53, 53, 51, 54, 101, 54, 58, 112, 105, 101, 99, 101, 115, 50, 48, 58, 92, 197, 230, 82, 190, 13, 230, 242, 120, 5, 179, 4, 100, 255, 155, 0, 244, 137, 240, 201, 55, 58, 112, 114, 105, 118, 97, 116, 101, 105, 49, 101, 101, 101];

    #[test]
    fn test_parse() {
        let sample_torrent_meta_info = MetaInfo {
            name: "sample.txt".to_string(),
            length: 20,
            announce: "udp://tracker.openbittorrent.com:80".to_string(),
            info_hash: [136, 250, 95, 136, 226, 2, 250, 155, 100, 55, 160, 172, 184, 222, 165, 99, 222, 40, 192, 120],
            pieces: vec![[92, 197, 230, 82, 190, 13, 230, 242, 120, 5, 179, 4, 100, 255, 155, 0, 244, 137, 240, 201]],
            piece_size: 65536,
            last_piece_size: 20,
            files: vec![
                FileInfo {
                    path: std::path::PathBuf::from("sample.txt"),
                    length: 20,
                    offset: 0
                }
            ]
        };

        assert_eq!(MetaInfo::try_from(SAMPLE_TORRENT).unwrap(), sample_torrent_meta_info);
    }
}
