use serde::{Deserialize, Serialize};

use crate::{io::store::Store, prelude::MetaInfo};

#[derive(Serialize, Deserialize)]
pub struct ResumeData {
	pub(crate) meta_info: MetaInfo,
	pub(crate) pieces: Vec<bool>,
	pub(crate) checksum: u64
}

pub trait Resume<S: Store> {
	type Error: std::error::Error;

	/// Attempt to resume from a [`ResumeData`]. If [`Ok`] is returned, the
	/// returned store MUST have the same checksum as the resume data.
	fn resume(self, resume_data: &ResumeData) -> Result<S, Self::Error>;
}
