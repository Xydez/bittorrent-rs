use crate::protocol::{
	extensions::Extensions,
	peer_id::{
		PeerId,
		Version
	}
};

/// Configuration of the session
#[derive(Debug)]
pub struct Configuration {
	/// Timeout when attempting to connect to a peer
	pub connect_timeout: std::time::Duration,
	/// Sever the connection if the peer doesn't send a message within this duration. Generally 2 minutes.
	pub alive_timeout: std::time::Duration,
	/// Size of a block. 16 KiB is the maximum permitted by the spec. Almost always 16 KiB.
	pub block_size: u32,
	/// The maximum amount of concurrent block downloads for a peer. Generally around 5-10 block downloads.
	pub concurrent_block_downloads: usize,
	/// The maximum number of active piece verification jobs.
	pub verification_jobs: usize,
	/// The maximum number of attempts to announce the torrent
	pub announce_retries: usize,
	/// Peer id of the session.
	pub peer_id: [u8; 20],
	/// Extensions supported by the session.
	pub extensions: Extensions
}

impl Default for Configuration {
	fn default() -> Configuration {
		Configuration {
			connect_timeout: std::time::Duration::from_secs_f64(10.0),
			alive_timeout: std::time::Duration::from_secs_f64(120.0),
			block_size: 16_384,
			concurrent_block_downloads: 10,
			verification_jobs: std::thread::available_parallelism()
				.map(std::num::NonZeroUsize::get)
				.unwrap_or(8),
			announce_retries: 5,
			// TODO: Change to something sensible. Apparently some clients close the connection if they can't parse the peer id. Should use Azureus style.
			peer_id: PeerId::generate(*b"bt", Version::new(0, 1, None, None)), //[b'x'; 20],
			extensions: Extensions([0; 8])
		}
	}
}
