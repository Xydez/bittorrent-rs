use protocol::metainfo::MetaInfo;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::store::Store;

#[derive(Error, Debug)]
pub enum ResumeError {
	#[error("An error occurred while reading or writing data")]
	IOError(#[from] std::io::Error),
	#[error("The checksum of the provided store does not match the deserialized store")]
	InvalidChecksum,
	#[error(transparent)]
	Other(#[from] Box<dyn std::error::Error>),
}

#[derive(Serialize, Deserialize)]
pub struct ResumeData {
	pub meta_info: MetaInfo,
	pub pieces: Vec<bool>,
	pub checksum: u64,
}

pub trait Resume<S: Store> {
	type Error: std::error::Error;

	/// Attempt to resume from a [`ResumeData`]. If [`Ok`] is returned, the
	/// returned store MUST have the same checksum as the resume data.
	fn resume(self, resume_data: &ResumeData) -> Result<S, Self::Error>;
}
