//! BitTorrent tracker implementation
//!
//! The tracker keeps track of which peers are able to provide which pieces of a torrent
//!
//! A tracker announce sends event to the tracker and receives a list of possible peers to connect to
//!
//! # Examples
//!
//! ```rust,no_run
//! # use protocol::tracker::{Announce, Event, Tracker};
//! #
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let mut tracker = Tracker::new("https://example-tracker.com/announce");
//!
//! let announce = Announce {
//!     info_hash: [0; 20], // In reality, this MUST be a valid info hash
//!     peer_id: [b'x'; 20],
//!     ip: None,
//!     port: 8000,
//!     uploaded: 0,
//!     downloaded: 0,
//!     left: 0,
//!     event: Some(Event::Started)
//! };
//!
//! let response = tracker.announce(&announce).await;
//!
//! println!("{:#?}", response);
//! # }
//! ```

use std::{
	convert::TryFrom,
	net::{Ipv4Addr, SocketAddrV4},
	time::Instant,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
	#[error("Invalid announce url")]
	InvalidUrl,
	#[error("Failed to send request")]
	RequestError(#[from] reqwest::Error),
	#[error("Received an invalid response from the tracker")]
	InvalidResponse,
	#[error("The tracker responded with invalid bencode")]
	InvalidResponseEncoding(#[from] serde_bencode::Error),
	#[error("The tracker responded with an error: {0}")]
	TrackerError(String),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Event {
	Started,
	Completed,
	Stopped,
}

/// Announce message sent to the tracker to broadcast events and receive a list of possible peers to establish a connection with
#[derive(Debug)]
pub struct Announce {
	/// Info hash of the torrent being announced
	pub info_hash: [u8; 20],
	/// Peer id of the client
	pub peer_id: [u8; 20],
	/// Optional IP address of the client, otherwise assumed to be the IP address from which the request came
	pub ip: Option<std::net::Ipv4Addr>,
	/// Port the client is listening on, typically in the range 6881-6889
	pub port: u16,
	/// Total amount of bytes uploaded since the client sent [`Event::Started`] to the tracker
	pub uploaded: usize,
	/// Total amount of bytes downloaded since the client sent [`Event::Started`] to the tracker
	pub downloaded: usize,
	/// Total amount of bytes the client has left to download until all of the torrent's pieces are downloaded
	pub left: usize,
	/// Event sent to the tracker
	///
	/// Must be one of
	/// * [Event::Started] on the first announce sent to the tracker
	/// * [Event::Stopped] when the client is shutting down
	/// * [Event::Completed] when all of the pieces of a torrent have been downloaded
	/// or [None] if it is an event-less request informing the tracker of the client's progress and updating the peer list
	pub event: Option<Event>,
}

/// BitTorrent tracker
#[derive(Debug)]
pub struct Tracker {
	/// HTTP client used to send requests
	client: reqwest::Client,
	/// URL to send the announce request to
	announce_url: String,
	/// Optional field containing the time and response of last announce that was sent to the tracker
	last_announce: Option<(Instant, Response)>,
	/// Optional tracker id received from the tracker
	tracker_id: Option<String>,
}

impl Tracker {
	/// Creates a new tracker instance
	///
	/// Calling this function does not establish a connection with the tracker
	pub fn new(announce: &str) -> Self {
		Tracker {
			client: reqwest::Client::new(),
			announce_url: announce.to_string(),
			last_announce: None,
			tracker_id: None,
		}
	}

	/// Send an announce request to the tracker
	pub async fn announce(&mut self, announce: &Announce) -> Result<Response> {
		// TODO: We currently only support compact mode, maybe have raw::Response and raw::CompactResponse?
		let mut query = vec![
			("port", announce.port.to_string()),
			("uploaded", announce.uploaded.to_string()),
			("downloaded", announce.downloaded.to_string()),
			("left", announce.left.to_string()),
			("compact", "1".to_string()),
		];

		if let Some(tracker_id) = &self.tracker_id {
			query.push(("trackerid", tracker_id.clone()));
		}

		if let Some(event) = &announce.event {
			query.push((
				"event",
				match event {
					Event::Started => "started",
					Event::Completed => "completed",
					Event::Stopped => "stopped",
				}
				.to_string(),
			));
		}

		// Note: We need to set info_hash and peer_id here because the params are automatically encoded, and we don't want to encode them twice
		let url = format!(
			"{}?info_hash={}&peer_id={}",
			self.announce_url,
			percent_encoding::percent_encode(
				&announce.info_hash,
				percent_encoding::NON_ALPHANUMERIC
			),
			percent_encoding::percent_encode(&announce.peer_id, percent_encoding::NON_ALPHANUMERIC)
		);

		let url = reqwest::Url::parse(&url).map_err(|_| Error::InvalidUrl)?;

		if !matches!(url.scheme(), "http" | "https") {
			return Err(Error::InvalidUrl);
		}

		let response_bytes = self
			.client
			.get(url)
			.query(&query)
			.send()
			.await?
			.error_for_status()?
			.bytes()
			.await?;

		let response_raw = serde_bencode::from_bytes::<raw::CompactResponse>(&response_bytes)?;

		if response_raw.tracker_id.is_some() {
			self.tracker_id = response_raw.tracker_id;
		}

		let response = match response_raw.failure_reason {
			Some(reason) => Err(Error::TrackerError(reason)),
			None => Ok(Response {
				interval: std::time::Duration::from_secs(
					response_raw.interval.ok_or(Error::InvalidResponse)? as u64,
				),
				min_interval: response_raw
					.min_interval
					.map(|seconds| std::time::Duration::from_secs(seconds as u64)),
				peers_addrs: response_raw
					.peers
					.ok_or(Error::InvalidResponse)?
					.chunks_exact(6)
					.map(|chunk| {
						<[u8; 6]>::try_from(chunk)
							.map_err(|_| Error::InvalidResponse)
							.map(|chunk| {
								SocketAddrV4::new(
									Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]),
									u16::from_be_bytes([chunk[4], chunk[5]]),
								)
							})
					})
					.collect::<Result<Vec<std::net::SocketAddrV4>>>()?,
			}),
		};

		if response.is_ok() {
			self.last_announce = Some((Instant::now(), response.as_ref().unwrap().clone()));
		}

		response
	}

	/// Returns the URL announce requests are sent to
	pub fn announce_url(&self) -> &str {
		&self.announce_url
	}

	pub fn last_announce(&self) -> &Option<(Instant, Response)> {
		&self.last_announce
	}
}

#[derive(Debug, Clone)]
pub struct Response {
	/// Interval in seconds the client SHOULD wait between sending event-less requests to the tracker
	pub interval: std::time::Duration,
	/// Minimum interval in seconds the client MUST wait between sending event-less requests to the tracker
	pub min_interval: Option<std::time::Duration>,
	/// List of peers the client MAY connect to
	pub peers_addrs: Vec<std::net::SocketAddrV4>,
}

mod raw {
	use serde::Deserialize;

	/// Compact tracker response
	#[derive(Debug, Deserialize)]
	pub struct CompactResponse {
		/// If a tracker response has a key failure reason, then that maps to a human readable string which explains why the query failed, and no other keys are required.
		#[serde(rename = "failure reason")]
		pub failure_reason: Option<String>,
		/// Interval in seconds the client SHOULD wait between sending event-less requests to the tracker
		pub interval: Option<usize>,
		/// Minimum interval in seconds the client MUST wait between sending event-less requests to the tracker
		#[serde(rename = "min interval")]
		pub min_interval: Option<usize>,
		/// List of dictionaries corresponding to peers
		pub peers: Option<serde_bytes::ByteBuf>,
		/// A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
		#[serde(rename = "tracker id")]
		pub tracker_id: Option<String>,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	#[ignore = "uses network"]
	async fn test_announce() {
		let mut tracker = Tracker::new("http://bttracker.debian.org:6969/announce");

		let info_hash: [u8; 20] = [
			154, 90, 140, 217, 173, 154, 190, 9, 16, 112, 10, 41, 14, 168, 97, 130, 46, 160, 90, 10,
		];

		tracker
			.announce(&Announce {
				info_hash,
				peer_id: [0xff; 20],
				ip: None,
				port: 8000,
				uploaded: 0,
				downloaded: 0,
				left: 0,
				event: None,
			})
			.await
			.unwrap();
	}
}
