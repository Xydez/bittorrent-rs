use std::{convert::TryFrom, net::{Ipv4Addr, SocketAddrV4}};

#[derive(Debug)]
pub enum TrackerError {
	RequestError(reqwest::Error),
	InvalidResponse,
	InvalidResponseEncoding(serde_bencode::Error),
	TrackerError(String)
}

impl From<reqwest::Error> for TrackerError {
	fn from(error: reqwest::Error) -> Self {
		return TrackerError::RequestError(error);
	}
}

impl From<serde_bencode::Error> for TrackerError {
	fn from(error: serde_bencode::Error) -> Self {
		return TrackerError::InvalidResponseEncoding(error);
	}
}

impl std::fmt::Display for TrackerError {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		match self {
			TrackerError::RequestError(error) => write!(f, "Request error: {}", error),
			TrackerError::InvalidResponseEncoding(error) => write!(f, "Invalid tracker response encoding: {}", error),
			TrackerError::TrackerError(error) => write!(f, "Tracker error: {}", error),
			TrackerError::InvalidResponse => write!(f, "Invalid tracker response")
		}
	}
}

impl std::error::Error for TrackerError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		return match self {
			TrackerError::RequestError(error) => Some(error),
			TrackerError::InvalidResponseEncoding(error) => Some(error),
			_ => None
		};
	}
}

pub(crate) type Result<T> = std::result::Result<T, TrackerError>;

#[derive(Debug)]
pub enum Event {
	Started, Completed, Stopped
}

#[derive(Debug)]
pub struct Announce {
	pub info_hash: [u8; 20],
	pub peer_id: [u8; 20],
	pub ip: Option<std::net::Ipv4Addr>,
	pub port: u16,
	pub uploaded: usize,
	pub downloaded: usize,
	pub left: usize,
	pub event: Option<Event>
}

#[derive(Debug)]
pub struct Tracker {
	client: reqwest::Client,
	announce: String
}

impl Tracker {
	pub fn new(announce: &str) -> Self {
		return Tracker {
			client: reqwest::Client::new(),
			announce: announce.to_string()
		};
	}

	/// Send an announce to the tracker
	pub async fn announce(&self, announce: &Announce) -> Result<Response> {
		// let peer_id = announce.peer_id.iter().collect::<String>().into_bytes();

		// Note: We currently only support compact mode, maybe have raw::Response and raw::CompactResponse?
		let mut query = vec![
			("port", announce.port.to_string()),
			("uploaded", announce.uploaded.to_string()),
			("downloaded", announce.downloaded.to_string()),
			("left", announce.left.to_string()),
			("compact", "1".to_string())
		];

		if let Some(event) = &announce.event {
			query.push(("event", match event {
				Event::Started => "started",
				Event::Completed => "completed",
				Event::Stopped => "stopped"
			}.to_string()));
		}

		// Note: We need to set info_hash and peer_id here because the params are automatically encoded, and we don't want to encode them twice
		let url = format!("{}?info_hash={}&peer_id={}", self.announce, percent_encoding::percent_encode(&announce.info_hash, percent_encoding::NON_ALPHANUMERIC), percent_encoding::percent_encode(&announce.peer_id, percent_encoding::NON_ALPHANUMERIC));

		let response_bytes = self.client.get(url)
			.query(&query)
			.send()
			.await?
			.error_for_status()?
			.bytes()
			.await?;

		let response = serde_bencode::from_bytes::<raw::Response>(&response_bytes)?;

		return match response.failure_reason {
			Some(reason) => Err(TrackerError::TrackerError(reason)),
			None => Ok(Response {
				interval: response.interval.ok_or(TrackerError::InvalidResponse)?,
				peers_addrs: response.peers
					.ok_or(TrackerError::InvalidResponse)?
					.chunks_exact(6)
					.map(|chunk| <[u8; 6]>::try_from(chunk)
						.map_err(|_| TrackerError::InvalidResponse)
						.map(|chunk|
							SocketAddrV4::new(Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]), u16::from_be_bytes([chunk[4], chunk[5]]))
						)
					)
					.collect::<Result<Vec<std::net::SocketAddrV4>>>()?
			})
		};
	}

	pub fn announce_url(&self) -> String {
		self.announce.clone()
	}
}

#[derive(Debug)]
pub struct Response {
	pub interval: usize,
	pub peers_addrs: Vec<std::net::SocketAddrV4>
}

mod raw {
	use serde::Deserialize;

	#[derive(Debug, Deserialize)]
	pub struct Response {
		/// If a tracker response has a key failure reason, then that maps to a human readable string which explains why the query failed, and no other keys are required.
		#[serde(rename = "failure reason")]
		pub failure_reason: Option<String>,
		/// Maps to the number of seconds the downloader should wait between regular rerequests
		pub interval: Option<usize>,
		/// List of dictionaries corresponding to peers
		pub peers: Option<serde_bytes::ByteBuf>
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	#[cfg_attr(not(feature = "network-tests"), ignore)]
	async fn test_announce() {
		let tracker = Tracker::new("http://bttracker.debian.org:6969/announce");

		let info_hash: [u8; 20] = [154, 90, 140, 217, 173, 154, 190, 9, 16, 112, 10, 41, 14, 168, 97, 130, 46, 160, 90, 10];

		tracker.announce(&Announce {
			info_hash,
			peer_id: ['x'; 20],
			ip: None,
			port: 8000,
			uploaded: 0,
			downloaded: 0,
			left: 0,
			event: None
		}).await.unwrap();
	}
}
