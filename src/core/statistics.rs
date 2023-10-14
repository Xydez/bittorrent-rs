use std::{collections::VecDeque, time::Instant};

// TODO: Maybe we should use a time-based solution instead
const TRACKED_EVENTS: usize = 64;

/// Torrent statistics
#[derive(Debug, Default)]
pub struct Statistics {
	/// Data received from peers in bytes
	downloaded: usize,
	/// Data sent to peers in bytes
	uploaded: usize,
	downloads: VecDeque<(Instant, usize)>,
	uploads: VecDeque<(Instant, usize)>,
}

impl Statistics {
	pub fn download(&mut self, bytes: usize) {
		Self::push_event(&mut self.downloads, bytes);
	}

	pub fn upload(&mut self, bytes: usize) {
		Self::push_event(&mut self.uploads, bytes);
	}

	pub fn downloaded(&self) -> usize {
		self.downloaded
	}

	pub fn uploaded(&self) -> usize {
		self.uploaded
	}

	pub fn download_rate(&self) -> Option<f64> {
		Self::calc_rate(&self.downloads)
	}

	pub fn upload_rate(&self) -> Option<f64> {
		Self::calc_rate(&self.uploads)
	}

	fn push_event(queue: &mut VecDeque<(Instant, usize)>, bytes: usize) {
		if queue.len() + 1 == TRACKED_EVENTS {
			queue.pop_front();
		}

		queue.push_back((Instant::now(), bytes));
	}

	fn calc_rate(queue: &VecDeque<(Instant, usize)>) -> Option<f64> {
		let time = (Instant::now() - queue.front()?.0).as_secs_f64();
		let bytes = queue.iter().fold(0, |acc, (_, bytes)| acc + bytes) as f64;

		Some(bytes / time)
	}
}
