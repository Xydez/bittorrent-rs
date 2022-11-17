/// Configuration of the session
pub struct Configuration {
	/// Timeout when attempting to connect to a peer
	pub connect_timeout: std::time::Duration,
	/// Sever the connection if the peer doesn't send a message within this duration. Generally 2 minutes.
	pub alive_timeout: std::time::Duration,
	/// Size of a block. 16 KiB is the maximum permitted by the spec. Almost always 16 KiB.
	pub block_size: u32,
	/// The maximum amount of concurrent block downloads for a peer. Generally 5 downloads.
	pub concurrent_block_downloads: usize,
	/// The maximum number of active piece verification jobs. Should be set to the `number of cores - 1` for optimal performance
	pub verification_jobs: usize,
	/// Peer id of the session
	pub peer_id: [u8; 20]
}

impl Default for Configuration {
	fn default() -> Configuration {
		Configuration {
			connect_timeout: std::time::Duration::from_secs_f64(8.0),
			alive_timeout: std::time::Duration::from_secs_f64(120.0),
			block_size: 16_384,
			concurrent_block_downloads: 5,
			verification_jobs: 4,
			// TODO: Change to something sensible. Apparently some clients close the connection if they can't parse the peer id. Should use Azureus style.
			peer_id: [b'x'; 20]
		}
	}
}
