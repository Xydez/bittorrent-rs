use std::{
	fs::{File, OpenOptions},
	hash::{Hash, Hasher},
	io::{Read, Seek, Write},
	path::{Path, PathBuf}
};

use crate::{core::torrent::ResumeData, protocol::metainfo::MetaInfo};

pub trait Store: std::fmt::Debug + Send {
	fn set(&mut self, piece: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
	fn get(&mut self, piece: usize) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>>;

	/// Calculate the checksum of all existing pieces in the store
	fn checksum(&mut self) -> Result<u64, Box<dyn std::error::Error>>;
}

#[derive(Debug)]
pub struct NullStore;

impl Store for NullStore {
	fn get(&mut self, _piece: usize) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
		unimplemented!("NullStore cannot be read");
	}

	fn set(&mut self, _piece: usize, _data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
		Ok(())
	}

	fn checksum(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
		unimplemented!("NullStore cannot calculate a checksum");
	}
}

#[derive(Debug)]
pub struct MemoryStore {
	data: Vec<Option<Vec<u8>>>
}

impl MemoryStore {
	// TODO: use piece_size and last_piece_size to assert that the correct data size is used in set
	pub fn new(pieces: usize, _piece_size: usize, _last_piece_size: usize) -> MemoryStore {
		MemoryStore {
			data: vec![None; pieces]
		}
	}
}

impl Store for MemoryStore {
	fn get(&mut self, piece: usize) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
		Ok(self.data[piece].clone())
	}

	fn set(&mut self, piece: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
		self.data[piece] = Some(data.to_vec());

		Ok(())
	}

	fn checksum(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
		let mut hasher = std::collections::hash_map::DefaultHasher::new();

		for piece in self.data.iter().filter_map(|val| val.as_ref()) {
			piece.hash(&mut hasher);
		}

		Ok(hasher.finish())
	}
}

#[derive(Debug)]
pub struct FileStore {
	piece_length: usize,
	pub(crate) directory: PathBuf,
	/// Opened files within the store and path not including base directory
	files: Vec<(usize, PathBuf, File)>,
	pub(crate) has_pieces: Vec<bool>
}

impl FileStore {
	/// Creates a file store using the given piece length and files
	///
	/// The total length is calculated from the sum of the file sizes
	pub fn new(
		piece_length: usize,
		directory: impl AsRef<Path>,
		files: impl IntoIterator<Item = (usize, PathBuf)>
	) -> std::io::Result<FileStore> {
		let files = files
			.into_iter()
			.map(|(length, path)| {
				let full_path = std::path::Path::new(directory.as_ref()).join(&path);

				(
					length,
					path,
					OpenOptions::new()
						.read(true)
						.write(true)
						.create(true)
						.open(full_path)
				)
			})
			.map(|(length, path, result)| result.map(|value| (length, path, value)))
			.collect::<std::io::Result<Vec<_>>>()?;

		let has_pieces = vec![
			false;
			files
				.iter()
				.map(|(length, _, _)| length)
				.sum::<usize>()
				.div_ceil(piece_length)
		];

		Ok(FileStore {
			piece_length,
			directory: directory.as_ref().to_path_buf(),
			files,
			has_pieces
		})
	}

	/// Creates a file store in the given directory using the provided [`MetaInfo`]
	pub fn from_meta_info(
		directory: impl AsRef<Path>,
		meta_info: &MetaInfo
	) -> std::io::Result<FileStore> {
		FileStore::new(
			meta_info.piece_size,
			directory,
			meta_info
				.files
				.iter()
				.map(|file| (file.length, file.path.clone()))
		)
	}

	#[cfg(feature = "resume")]
	pub fn resume(
		directory: impl AsRef<Path>,
		resume_data: ResumeData
	) -> Result<FileStore, crate::core::torrent::ResumeError> {
		use crate::core::torrent::ResumeError;

		let mut store = Self::from_meta_info(directory, resume_data.meta_info)?;
		store.has_pieces = resume_data.pieces.clone();

		if resume_data.checksum != store.checksum()? {
			Err(ResumeError::InvalidChecksum)
		} else {
			Ok(store)
		}
	}

	pub fn destroy(self) -> std::io::Result<()> {
		for (_, path, _) in self.files {
			std::fs::remove_file(self.directory.join(path))?;
		}

		Ok(())
	}

	fn size(&self) -> usize {
		self.files.iter().map(|(length, _, _)| length).sum()
	}

	fn file_of_byte(&self, index: usize) -> Option<usize> {
		let mut total = 0;

		for (i, (length, _, _)) in self.files.iter().enumerate() {
			total += length;

			if total > index {
				return Some(i);
			}
		}

		None
	}

	fn file_bytes(&self, index: usize) -> (usize, usize) {
		let mut begin = 0;

		for i in 0..index {
			let (length, _, _) = self.files[i];
			begin += length
		}

		let (length, _, _) = self.files[index];

		let end = begin + length;

		(begin, end)
	}
}

impl Store for FileStore {
	fn set(&mut self, piece: usize, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
		assert!(data.len() <= self.piece_length);

		let mut remaining_bytes = data.len();

		while remaining_bytes > 0 {
			// Index of the byte we are writing
			let index = piece * self.piece_length + data.len() - remaining_bytes;
			let file_index = self.file_of_byte(index).unwrap_or_else(|| {
				panic!(
					"Failed to set piece {}. Byte is out of bounds ({} >= {})",
					piece,
					index,
					self.size()
				)
			});
			let (begin, end) = self.file_bytes(file_index);

			let bytes_to_write = (end - index).min(remaining_bytes);

			let (_, _, file) = &mut self.files[file_index];
			file.seek(std::io::SeekFrom::Start(index as u64 - begin as u64))?;
			file.write_all(
				&data[(data.len() - remaining_bytes)
					..(data.len() - remaining_bytes + bytes_to_write)]
			)?;

			remaining_bytes -= bytes_to_write;
		}

		self.has_pieces[piece] = true;

		Ok(())
	}

	fn get(&mut self, piece: usize) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
		let mut data = Vec::with_capacity(self.piece_length);

		if !self.has_pieces[piece] {
			return Ok(None);
		}

		while data.len() < self.piece_length {
			let remaining_bytes = self.piece_length - data.len();
			let index = (piece + 1) * self.piece_length - remaining_bytes;
			let file_index = if let Some(index) = self.file_of_byte(index) {
				index
			} else {
				// If the index is None we are reading the last piece
				break;
			};

			let (begin, end) = self.file_bytes(file_index);
			let bytes_to_read = (end - index).min(remaining_bytes);

			let (_, _, file) = &mut self.files[file_index];
			file.seek(std::io::SeekFrom::Start(index as u64 - begin as u64))?;

			let mut buf = vec![0; bytes_to_read];
			file.read_exact(&mut buf)?;

			data.extend(buf.into_iter());
		}

		Ok(Some(data))
	}

	fn checksum(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
		let mut hasher = std::collections::hash_map::DefaultHasher::new();

		for i in 0..self.has_pieces.len() {
			if let Some(data) = self.get(i)? {
				data.hash(&mut hasher);
			}
		}

		Ok(hasher.finish())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	#[cfg_attr(not(feature = "io-tests"), ignore)]
	fn single_file() {
		let mut store =
			FileStore::new(7, ".test_dir", vec![(21, "test_single_file.txt".into())]).unwrap();

		for i in 0..3 {
			store.set(i, b"testing").unwrap();
		}

		for i in 0..3 {
			assert_eq!(store.get(i).unwrap(), Some(b"testing".to_vec()));
		}

		store.destroy().unwrap();
	}

	#[test]
	#[cfg_attr(not(feature = "io-tests"), ignore)]
	fn single_file_last_piece() {
		let mut store = FileStore::new(
			7,
			".test_dir",
			vec![(18, "test_single_file_last_piece.txt".into())]
		)
		.unwrap();

		for i in 0..2 {
			store.set(i, b"testing").unwrap();
		}

		store.set(2, b"test").unwrap();

		for i in 0..2 {
			assert_eq!(store.get(i).unwrap(), Some(b"testing".to_vec()));
		}

		assert_eq!(store.get(2).unwrap(), Some(b"test".to_vec()));

		store.destroy().unwrap();
	}

	#[test]
	#[cfg_attr(not(feature = "io-tests"), ignore)]
	fn multi_file() {
		let mut store = FileStore::new(
			7,
			".test_dir",
			vec![
				(10, "test_multi_file_a.txt".into()),
				(8, "test_multi_file_b.txt".into()),
			]
		)
		.unwrap();

		for i in 0..3 {
			assert_eq!(store.get(i).unwrap(), None);
		}

		for i in 0..2 {
			store.set(i, b"testing").unwrap();
		}

		store.set(2, b"test").unwrap();

		for i in 0..2 {
			assert_eq!(store.get(i).unwrap(), Some(b"testing".to_vec()));
		}

		assert_eq!(store.get(2).unwrap(), Some(b"test".to_vec()));

		store.destroy().unwrap();
	}
}
