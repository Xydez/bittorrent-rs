use std::convert::TryFrom;

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
	pub peer_id: [char; 20],
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

	pub async fn announce(&self, announce: &Announce) -> Result<Response> {
		let bytes = announce.peer_id.iter().collect::<String>().into_bytes();

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

		// We need to set info_hash and peer_id here because the `query` params are automatically encoded, and we don't want to encode them twice
		let url = format!("{}?info_hash={}&peer_id={}", self.announce, percent_encoding::percent_encode(&announce.info_hash, percent_encoding::NON_ALPHANUMERIC), percent_encoding::percent_encode(&bytes, percent_encoding::NON_ALPHANUMERIC));

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
				peers: response.peers
					.ok_or(TrackerError::InvalidResponse)?
					.chunks_exact(6)
					.map(|chunk| <[u8; 6]>::try_from(chunk)
						.map_err(|_| TrackerError::InvalidResponse)
						.map(|chunk| Peer {
							ip: std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]),
							port: u16::from_le_bytes([chunk[4], chunk[5]])
						})
					)
					.collect::<Result<Vec<Peer>>>()?
			})
		};
	}
}

#[derive(Debug)]
pub struct Response {
	pub interval: usize,
	pub peers: Vec<Peer>
}

#[derive(Debug)]
pub struct Peer {
	/// IP address or dns name as a string
	pub ip: std::net::Ipv4Addr,
	/// Port number
	pub port: u16
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
		#[serde(with = "serde_bytes")]
		pub peers: Option<Vec<u8>>
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_announce() {
		let tracker = Tracker::new("http://bttracker.debian.org:6969/announce");

		let info_hash: [u8; 20] = [121, 146, 200, 2, 127, 106, 243, 152, 107, 62, 156, 53, 218, 159, 100, 231, 10, 242, 244, 240];

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
