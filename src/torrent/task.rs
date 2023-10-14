//! # Research
//!
//! ## [Rarest First and Choke Algorithms Are Enough - Section 2.2](http://conferences.sigcomm.org/imc/2006/papers/p20-legout.pdf)
//!
//! ### Peer set
//! If ever the peer set size of a peer falls below a predefined threshold,
//! typically 20 peers, this peer will contact the tracker again to obtain a
//! new list of IP addresses of peers. By default, the maximum peer set size is
//! 80.
//!
//! Moreover, a peer should not exceed a threshold of 40 initiated connections
//! among the 80 at each time. As a consequence, the 40 remaining connections
//! should be initiated by remote peers. This policy guarantees a good
//! interconnection among the peer sets in the torrent.
//!
//! ### Block priority
//! BitTorrent also applies a strict priority policy, which is at the block
//! level. When at least one block of a piece has been requested, the other
//! blocks of the same piece are requested with the highest priority. The aim
//! of the strict priority policy is to complete the download of a piece as
//! fast as possible. As only complete pieces can be sent, it is important to
//! minimize the number of partially received pieces.
//!
//! ### End game
//! Finally, the last policy is the end game mode. This mode starts once a
//! peer has requested all blocks, i.e., all blocks have either been already
//! received or requested. While in this mode, the peer requests all blocks not
//! yet received to all the peers in its peer set that have the corresponding
//! blocks. Each time a block is received, it cancels the request for the
//! received block to all the peers in its peer set that have the corresponding
//! pending request. As a peer has a small buffer of pending requests, all
//! blocks are effectively requested close to the end of the download.
//! Therefore, the end game mode is used at the very end of the download, thus
//! it has little impact on the overall performance.
//!
//! ## [Bittorrent Protocol Specification v1.0 (unofficial)](https://wiki.theory.org/BitTorrentSpecification)
//!
//! ### Listener
//! Ports reserved for BitTorrent are typically 6881-6889. Clients may choose
//! to give up if it cannot establish a port within this range.

use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
};

use common::util;
use log::{error, info, trace, warn};
use protocol::{
	tracker::{self, Announce, Response, Tracker},
	wire::connection::{Handshake, Wire},
};
use rand::Rng;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::task::JoinMap;

use crate::{
	core::{
		configuration::Configuration,
		event::{Event, PeerEvent, TorrentEvent},
	},
	peer::{self, Peer, PeerHandle},
	session::{EventSender, TorrentPtr},
	torrent::WorkerId,
};

#[derive(Error, Debug)]
pub enum Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Command {
	/// The torrent task is shutting down
	Shutdown,
	/// The torrent task has completed downloading the torrent
	Complete,
}

pub type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
pub type CommandReceiver = tokio::sync::mpsc::UnboundedReceiver<Command>;

type PeerSender = tokio::sync::mpsc::UnboundedSender<(Peer, Option<OwnedSemaphorePermit>)>;
type PeerReceiver = tokio::sync::mpsc::UnboundedReceiver<(Peer, Option<OwnedSemaphorePermit>)>;

pub async fn run(
	torrent: TorrentPtr,
	mut cmd_rx: CommandReceiver,
	event_tx: EventSender,
	(conn_tx, mut conn_rx): (PeerSender, PeerReceiver),
	config: Arc<Configuration>,
) -> Result<()> {
	// Map of a worker's id to handle
	let mut worker_data: HashMap<WorkerId, (PeerHandle, Option<OwnedSemaphorePermit>)> =
		HashMap::new();

	// JoinMap of a worker's id to the result of the process
	let mut worker_set: JoinMap<WorkerId, peer::Result<()>> = JoinMap::new();

	// Channel used to receive events from peers
	let (torrent_event_tx, mut torrent_event_rx) = tokio::sync::mpsc::unbounded_channel();

	// Addresses that have not yet been contacted
	let mut addresses = HashSet::new();

	// Semaphore for maximum number of self-initiated connections
	let initiated_semaphore = Arc::new(Semaphore::new(config.max_initiated_peers));

	// 1. Announce that the torrent is started
	let response = {
		let mut lock = torrent.lock().await;

		let announce = Announce {
			info_hash: torrent.meta_info.info_hash,
			peer_id: config.peer_id,
			ip: None,
			port: config.port,
			uploaded: 0,
			downloaded: 0,
			left: lock.left(),
			event: Some(tracker::Event::Started),
		};

		try_announce(&mut lock.state.tracker, announce, &config)
			.await
			.expect("Failed to perform started announce")
	};

	// 2. Add the received addresses
	addresses.extend(response.peers_addrs);

	// 3. Running state
	loop {
		// TODO: It is crucial that we try to avoid await points and delays as
		// much as possible in this loop in order to respond to events quickly
		// because we will be sending them on to session.

		let (last_announce_instant, last_announce_response) = torrent
			.lock()
			.await
			.state
			.tracker
			.last_announce()
			.as_ref()
			.unwrap()
			.clone();

		// If we have less than `config.max_initiated_peers` self-initiated
		// connections, attempt to connect to peers from `addresses`
		loop {
			// `initiated_sempahore` has `config.max_initiated_peers` permits
			// for self-initiated connections, and so if there are any
			// available permits, then we may connect to a new peer.

			if addresses.is_empty() {
				break;
			}

			let Ok(permit) = initiated_semaphore.clone().try_acquire_owned() else {
				break;
			};

			let address = *addresses
				.iter()
				.nth(rand::thread_rng().gen_range(0..addresses.len()))
				.unwrap();
			let conn_tx = conn_tx.clone();
			let config = config.clone();
			let info_hash = torrent.meta_info.info_hash;

			tokio::spawn(async move {
				let permit = permit;

				match Wire::connect(address).await {
					Ok(mut wire) => {
						if let Err(error) = wire
							.send_handshake(&Handshake::new(
								info_hash,
								config.peer_id,
								config.extensions,
							))
							.await
						{
							error!(
								"Failed to send handsake to peer: {}",
								util::error_chain(error)
							);
							return;
						};

						let handshake = match wire.receive_handshake().await {
							Ok(handshake) => handshake,
							Err(error) => {
								error!(
									"Failed to receive handshake from peer: {}",
									util::error_chain(error)
								);
								return;
							}
						};

						let peer = Peer::new(wire, handshake);

						conn_tx.send((peer, Some(permit))).unwrap();
					}
					Err(error) => {
						trace!(
							"An error occurred while establishing a connection with a peer: {}",
							util::error_chain(error)
						);
					}
				}
			});
		}

		// If ever the peer set size of a peer falls below a predefined
		// threshold, typically 20 peers, this peer will contact the tracker
		// again to obtain a new list of IP addresses of peers. By default, the
		// maximum peer set size is 80.
		let announce_interval = if addresses.len()
			+ (config.max_initiated_peers - initiated_semaphore.available_permits())
			+ worker_set.len()
			< 20
		{
			response.interval.min(
				last_announce_response
					.min_interval
					.unwrap_or(config.announce_min_interval),
			)
		} else {
			response.interval
		};

		tokio::select! {
			// 1. Handle commands
			command = cmd_rx.recv() => {
				match command.unwrap() {
					super::Command::Shutdown => {
						// Drops all worker handles
						worker_data.clear();

						// Join all workers
						while let Some((worker_id, ret)) = worker_set.join_next().await {
							if let Err(err) = ret.expect("worker panicked") {
								error!("worker {worker_id} joined with an error: {}", util::error_chain(err));
							} else {
								trace!("worker {worker_id} joined");
							}
						}

						break;
					}
					super::Command::Complete => {
						let mut lock = torrent.lock().await;

						if lock.left() > 0 {
							warn!("Command::Complete received even though there are {} bytes left until completion", lock.left());
						}

						let announce = Announce {
							info_hash: torrent.meta_info.info_hash,
							peer_id: config.peer_id,
							ip: None,
							port: config.port,
							uploaded: lock.state.stats.uploaded(),
							downloaded: lock.state.stats.downloaded(),
							left: lock.left(),
							event: Some(tracker::Event::Started)
						};

						try_announce(&mut lock.state.tracker, announce, &config).await.expect("Failed to perform stopped announce");
					}
				}
			},
			// 2. Join workers
			Some((worker_id, ret)) = worker_set.join_next() => {
				// Remove from workers (eventual permit gets freed automatically)
				let (peer_handle, _permit) = worker_data.remove(&worker_id).unwrap();
				let pid = peer_handle.data().lock().await.peer_id_short();

				if let Err(err) = ret.expect("worker panicked") {
					error!("Worker {worker_id} for peer {pid} joined early ({} total peers). Error: {}", worker_set.len(), util::error_chain(err));
				} else {
					warn!("worker {worker_id} for peer {pid} joined early ({} total peers)", worker_set.len());
				}
			},
			// 3. Start workers on successful connections
			peer = conn_rx.recv() => {
				let (peer, permit_opt) = peer.unwrap();

				let pid = peer.peer_id_short();
				let worker_id = peer.id();

				let mode = peer::task::Mode {
					download: true,
					seed: false
				};

				let (handle, task) = match peer.spawn(mode, torrent.clone(), torrent_event_tx.clone(), config.clone()).await {
					Ok(val) => val,
					Err(error) => {
						error!("Failed to establish protocol with peer {pid}. Error: {}", util::error_chain(error));
						continue;
					}
				};

				worker_set.spawn(worker_id, task);
				worker_data.insert(worker_id, (handle, permit_opt));

				info!("Connected to peer {pid} ({} peers)", worker_set.len());
			}

			// 4. Find new peers (must also adhere to min_interval / some config setting)
			_ = tokio::time::sleep_until((last_announce_instant + announce_interval).into()) => {
				let response = {
					let mut torrent = torrent.lock().await;

					let announce = Announce {
						info_hash: torrent.torrent.meta_info.info_hash,
						peer_id: config.peer_id,
						ip: None,
						port: config.port,
						uploaded: torrent.state.stats.uploaded(),
						downloaded: torrent.state.stats.downloaded(),
						left: torrent.left(),
						event: Some(tracker::Event::Started)
					};

					try_announce(&mut torrent.state.tracker, announce, &config).await.expect("Failed to perform regular announce")
				};

				addresses.extend(response.peers_addrs);
			},
			// 5. Receive events from the peer tasks
			event = torrent_event_rx.recv() => {
				let (worker_id, event) = event.unwrap();

				match event {
					PeerEvent::BlockReceived(piece_id, block) | PeerEvent::BlockSent(piece_id, block) => {
						let mut torrent = torrent.lock().await;
						let size = torrent.torrent.meta_info.size_of_block(piece_id, block, config.block_size);

						match event {
							PeerEvent::BlockReceived(_, _) => torrent.state.stats.download(size),
							PeerEvent::BlockSent(_, _) => torrent.state.stats.upload(size),
							_ => unreachable!()
						}
					}
					_ => () // PeerEvent::Interested and PeerEvent::NotInterested
				}

				let event = Event::TorrentEvent(torrent.id, TorrentEvent::PeerEvent(worker_id, event));
				event_tx.send(event).unwrap();
			}
		}
	}

	// 4. Shutdown
	trace!("Torrent shutting down");

	// Announce shutdown
	{
		let mut lock = torrent.lock().await;

		let announce = Announce {
			info_hash: torrent.meta_info.info_hash,
			peer_id: config.peer_id,
			ip: None,
			port: config.port,
			uploaded: lock.state.stats.uploaded(),
			downloaded: lock.state.stats.downloaded(),
			left: lock.left(),
			event: Some(tracker::Event::Started),
		};

		try_announce(&mut lock.state.tracker, announce, &config)
			.await
			.expect("Failed to perform stopped announce");
	};

	Ok(())
}

async fn try_announce(
	tracker: &mut Tracker,
	announce: Announce,
	config: &Configuration,
) -> Option<Response> {
	for i in 0..config.announce_attempts {
		info!(
			"Announcing to tracker \"{}\" (attempt {}/{})",
			tracker.announce_url(),
			i + 1,
			config.announce_attempts
		);

		match tracker.announce(&announce).await {
			Ok(response) => {
				info!("Announce success: {response:#?}");

				return Some(response);
			}
			Err(error) => {
				error!("Failed to announce: {}", util::error_chain(&error));

				if let tracker::Error::InvalidUrl = error {
					return None;
				}

				tokio::time::sleep(config.announce_retry_delay).await;
			}
		}
	}

	None
}
