use rand::Rng;

use crate::protocol::{
	extensions::Extensions,
	peer_id::{PeerId, Version}
};

/// Configuration of the session
#[derive(Debug)]
pub struct Configuration {
	/// Timeout when attempting to connect to a peer.
	///
	/// Default: 10 seconds
	pub connect_timeout: std::time::Duration,
	/// Maximum amount of active peer connections. Should generally not exceed 80 connections.
	///
	/// Default: `80`
	pub max_peer_connections: usize,
	/// Maximum amount of self-initiated peer connections.
	/// * MUST be less than or equal to `max_peer_connections`.
	/// * SHOULD be 50% of `max_peer_connections`.
	///
	/// Default: `40`
	pub max_initiated_peers: usize,
	/// Sever the connection if the peer doesn't send a message within this duration.
	///
	/// Default: 2 minutes
	pub alive_timeout: std::time::Duration,
	/// Size of a block in bytes.
	/// * MUST be less than 16 KiB, as it is the maximum permitted by the spec.
	/// * SHOULD be exactly 16 KiB as it is best for performance and almost
	///   always used by other peers.
	///
	/// Default: `16384` bytes
	pub block_size: usize,
	/// The maximum amount of concurrent block downloads for a peer. Generally around 5-10 block downloads.
	///
	/// Default: `10`
	pub concurrent_block_downloads: usize,
	/// The maximum number of active piece verification jobs.
	/// * SHOULD NOT be more than the parallelism provided by the operating system.
	///
	/// Default: `std::thread::available_parallelism` or `8`
	pub verification_jobs: usize,
	/// The maximum number of attempts to announce the torrent.
	///
	/// Default: `5`
	pub announce_attempts: usize,
	/// The delay between announce retries.
	///
	/// Default: 5 seconds
	pub announce_retry_delay: std::time::Duration,
	/// The minimum interval the client waits between sending event-less requests to the tracker.
	///
	/// Default: 30 seconds
	pub announce_min_interval: std::time::Duration,
	/// The number of pieces to download using a random-first selection strategy in order to create an initial set of pieces to bargain with before switching to a rarest-first strategy.
	///
	/// Default: `4`
	pub initial_pieces: usize,
	/// Peer id of the session.
	/// * SHOULD follow Azureus-style peer id conventions - apparrently some clients close the connection if they can't parse the peer id.
	///
	/// Default: Azureus-style peer id with the id "bt", the version, and randomly generated data
	pub peer_id: [u8; 20],
	/// Extensions supported by the session.
	///
	/// Default: `Extensions([0u8; 8])`
	pub extensions: Extensions,
	/// Port used by client.
	/// * SHOULD be within `6881..=6889` as clients MAY choose to give up if the port is not within this range
	///
	/// Default: Random number in the range `6881..=6889`
	pub port: u16
}

impl Default for Configuration {
	fn default() -> Configuration {
		Configuration {
			connect_timeout: std::time::Duration::from_secs_f64(10.0),
			max_peer_connections: 80,
			max_initiated_peers: 40,
			alive_timeout: std::time::Duration::from_secs_f64(120.0),
			block_size: 16_384,
			concurrent_block_downloads: 10,
			verification_jobs: std::thread::available_parallelism()
				.map(std::num::NonZeroUsize::get)
				.unwrap_or(8),
			announce_attempts: 5,
			announce_retry_delay: std::time::Duration::from_secs_f64(5.0),
			announce_min_interval: std::time::Duration::from_secs_f64(5.0),
			initial_pieces: 4,
			peer_id: PeerId::generate(*b"bt", Version::new(0, 1, None, None)), //[b'x'; 20],
			extensions: Extensions([0; 8]),
			port: rand::thread_rng().gen_range(6881..=6889)
		}
	}
}
