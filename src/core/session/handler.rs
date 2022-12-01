//! Event handler of [`Session`] for events in [`Event`]

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
	core::{
		event::{
			Event,
			PieceEvent,
			TorrentEvent
		},
		peer::Peer,
		piece,
		session::Session,
		torrent::WorkerHandle,
		util,
		worker::{
			self,
			Mode
		}
	},
	protocol::wire::{
		Handshake,
		Wire
	}
};

impl Session {
	pub async fn handle(&mut self, event: &Event) {
		let prefix = format!("{event}");

		match &event {
			Event::Started | Event::Stopped => {
				log::info!("{prefix}");
			}
			Event::TorrentEvent(torrent_id, event) => match event {
				TorrentEvent::Added => {
					log::info!("{prefix}");

					let torrent = self.torrent(*torrent_id);

					let torrent = torrent.clone(); // tmp
					let config = self.config.clone();
					let tx = self.tx.clone();

					tokio::spawn(async move {
						let response = torrent.write().await.try_announce(&config).await;

						if let Some(response) = response {
							tx.send(Event::TorrentEvent(
								torrent.read().await.id(),
								TorrentEvent::Announced(Arc::new(response))
							))
							.unwrap();
						} else {
							log::error!(
								"{prefix} | Torrent {} failed to announce",
								util::hex(&torrent.read().await.meta_info.info_hash)
							);
						}
					});
				}
				TorrentEvent::Announced(response) => {
					log::info!("{prefix}");

					let torrent = self.torrents[torrent_id].clone();

					let peer_id = self.config.peer_id;
					let info_hash = torrent.read().await.meta_info.info_hash;

					// Add all peers to the torrent
					for addr in response.peers_addrs.clone() {
						let torrent = torrent.clone();
						let tx = self.tx.clone();
						let config = self.config.clone();

						// TODO: Use a tokio::task::JoinSet instead
						tokio::spawn(async move {
							let stream = match tokio::time::timeout(
								config.connect_timeout,
								tokio::net::TcpStream::connect(&addr)
							)
							.await
							{
								Ok(Ok(stream)) => {
									log::debug!("Connected to {}", addr);
									stream
								}
								Ok(Err(error)) => {
									log::error!(
										"Failed to connect to {}. {}",
										addr,
										util::error_chain(error)
									);
									return;
								}
								Err(_) => {
									log::error!("Failed to connect to peer. Connection timed out.");
									return;
								}
							};

							let handshake = Handshake {
								extensions: config.extensions.clone(),
								info_hash,
								peer_id
							};

							let wire = Wire::new(stream);
							let peer = match tokio::time::timeout(
								config.connect_timeout,
								Peer::handshake(wire, handshake)
							)
							.await
							{
								Ok(Ok(peer)) => {
									log::info!(
										"Handshake performed with {}",
										peer //util::hex(peer.peer_id())
									);
									peer
								}
								Ok(Err(error)) => {
									log::error!(
										"Failed to perform handshake with address {}: {}",
										addr,
										util::error_chain(error)
									);
									return;
								}
								Err(_) => {
									log::error!("Failed to perform handshake with address {}: Connection timed out", addr);
									return;
								}
							};

							let peer = Arc::new(Mutex::new(peer));
							let (mode_tx, mode_rx) = tokio::sync::watch::channel(Mode {
								download: true,
								seed: false
							});

							let mut lock = torrent.write().await;

							let task = tokio::spawn(worker::run(
								config,
								torrent.clone(),
								peer.clone(),
								tx,
								mode_rx
							));

							lock.peers.push(WorkerHandle {
								mode_tx,
								peer,
								task
							});
						});
					}
				}
				TorrentEvent::PieceEvent(piece, event) => match event {
					PieceEvent::Block(block) => {
						let torrent = self.torrents[torrent_id].clone();

						let Some(download) = torrent.read().await.downloads.get(piece).cloned() else {
							// This probably means that the download succeeded but we did not cancel the piece
							log::warn!("{prefix} | Download for block {} not found, block downloaded in vain", util::fmt_block(*piece, *block));
							return;
						};

						let download_lock = download.lock().await;

						if let Some(data) = download_lock.data() {
							// Remove the piece download because it is done
							{
								let mut lock = torrent.write().await;
								lock.downloads.remove(piece);
								lock.pieces[*piece as usize].state = piece::State::Downloaded;
							}

							log::debug!("{prefix} | block={} | Final block received, emitting TorrentEvent::Downloaded", util::fmt_block(*piece, *block));

							self.tx
								.send(Event::TorrentEvent(
									*torrent_id,
									TorrentEvent::PieceEvent(
										*piece,
										PieceEvent::Downloaded(Arc::new(data))
									)
								))
								.unwrap();
						}
					}
					PieceEvent::Downloaded(data) => {
						log::debug!("{prefix} | Piece downloaded, starting verification worker");

						// TODO: Spawn a thread that verifies the data (verification workers)
						let semaphore = self.verification_semaphore.clone();
						let tx = self.tx.clone();
						let piece = *piece;
						let torrent = self.torrents[torrent_id].clone();
						let data = data.clone();

						tokio::spawn(async move {
							let _permit = semaphore.acquire_owned().await.unwrap();
							torrent.write().await.pieces[piece as usize].state =
								piece::State::Verifying;

							let hash = torrent.read().await.meta_info.pieces[piece as usize];
							//let intact = verify_piece(&data, &hash);
							let intact = util::async_verify(data.clone(), &hash).await;

							torrent.write().await.pieces[piece as usize].state = if intact {
								piece::State::Verified
							} else {
								piece::State::Pending // TODO: If we use Block we also need to reset the block state
							};

							if intact {
								tx.send(Event::TorrentEvent(
									torrent.read().await.id(),
									TorrentEvent::PieceEvent(piece, PieceEvent::Verified(data))
								))
								.unwrap();
							}
						});
					}
					PieceEvent::Verified(data) => {
						log::debug!("{prefix} | Piece verified, writing to store");

						// Store the piece
						let tx = self.tx.clone();
						let data = data.clone();
						let torrent = self.torrents[torrent_id].clone();
						let piece = *piece;

						let lock = torrent.read().await;
						//let piece_size = lock.meta_info.piece_size;
						let store = lock.store.clone();
						drop(lock);

						tokio::spawn(async move {
							tokio::task::spawn_blocking(move || {
								store
									.blocking_lock()
									.set(piece as usize, &data)
									.expect("Failed to write to store");
							})
							.await
							.unwrap();

							torrent.write().await.pieces[piece as usize].state = piece::State::Done;
							tx.send(Event::TorrentEvent(
								torrent.read().await.id(),
								TorrentEvent::PieceEvent(piece, PieceEvent::Done)
							))
							.unwrap();
						});
					} // TODO: (Maybe) push the data onto an io writer queue. How do we pass around the data?
					PieceEvent::Done => {
						if self.torrents[torrent_id].read().await.is_done() {
							self.tx
								.send(Event::TorrentEvent(*torrent_id, TorrentEvent::Done))
								.unwrap();
						}
					}
				},
				TorrentEvent::Done => log::info!("{prefix}")
			}
		}
	}
}
