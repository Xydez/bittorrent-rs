//! Event handler of [`Session`] for events in [`Event`]

use std::sync::Arc;

use crate::core::{
	event::{Event, PeerEvent, PieceEvent, TorrentEvent},
	piece,
	session::Session,
	torrent::TorrentId,
	util
};

pub async fn handle(session: &mut Session, event: &Event) {
	let prefix = format!("{event}");

	match &event {
		Event::Started | Event::Stopped => log::info!("{prefix}"),
		Event::TorrentEvent(torrent_id, event) => {
			handle_torrent_event(session, prefix, *torrent_id, event).await
		}
	}
}

async fn handle_torrent_event(
	session: &Session,
	prefix: String,
	torrent_id: TorrentId,
	event: &TorrentEvent
) {
	match event {
		// TODO: Useless log messages
		TorrentEvent::Added => {
			log::warn!("added, do we keep this?");
		}
		TorrentEvent::Announced(_response) => {
			log::warn!("announced, do we keep this?");
		}
		TorrentEvent::PeerEvent(_peer, PeerEvent::BlockReceived(piece, block)) => {
			let torrent = &session.torrents[&torrent_id].torrent;

			let Some(download) = torrent.lock().await.state().downloads.get(piece).cloned() else {
				// This probably means that the download succeeded but we did not cancel the piece
				log::warn!("{prefix} | Download for block {} not found, block downloaded in vain", util::fmt_block(*piece, *block));
				return;
			};

			let download_lock = download.lock().await;

			if let Some(data) = download_lock.data() {
				// Remove the piece download because it is done
				{
					let mut lock = torrent.lock().await;
					lock.state_mut().downloads.remove(piece);
					//lock.state_mut().pieces[*piece as usize].state = piece::State::Verifying;
				}

				log::debug!(
					"{prefix} | block={} | Final block received, emitting TorrentEvent::Downloaded",
					util::fmt_block(*piece, *block)
				);

				session
					.tx
					.send(Event::TorrentEvent(
						torrent_id,
						TorrentEvent::PieceEvent(*piece, PieceEvent::Downloaded(Arc::new(data)))
					))
					.unwrap();
			}
		}
		TorrentEvent::PieceEvent(piece, event) => match event {
			PieceEvent::Downloaded(data) => {
				log::debug!("{prefix} | Piece downloaded, starting verification worker");

				let semaphore = session.verification_semaphore.clone();
				let tx = session.tx.clone();
				let piece = *piece;
				let torrent = session.torrents[&torrent_id].torrent.clone();
				let data = data.clone();

				tokio::spawn(async move {
					let _permit = semaphore.acquire_owned().await.unwrap();
					torrent.lock().await.state_mut().pieces[piece as usize].state =
						piece::State::Verifying;

					let hash = torrent.meta_info.pieces[piece as usize];
					let intact = util::async_verify(data.clone(), &hash).await;

					if intact {
						tx.send(Event::TorrentEvent(
							torrent.id,
							TorrentEvent::PieceEvent(piece, PieceEvent::Verified(data))
						))
						.unwrap();
					} else {
						torrent.lock().await.state_mut().pieces[piece as usize].state =
							piece::State::Pending;
					}
				});
			}
			PieceEvent::Verified(data) => {
				log::debug!("{prefix} | Piece verified, writing to store");

				// Store the piece
				let tx = session.tx.clone();
				let data = data.clone();
				let torrent = session.torrents[&torrent_id].torrent.clone();
				let piece = *piece;

				// TODO: (Maybe) push the data onto an io writer queue or similar?
				tokio::spawn(async move {
					{
						let torrent = torrent.clone();

						tokio::task::spawn_blocking(move || {
							torrent
								.store
								.blocking_lock()
								.set(piece as usize, &data)
								.expect("Failed to write to store")
						})
						.await
						.unwrap();
					}

					torrent.lock().await.state_mut().pieces[piece as usize].state =
						piece::State::Done;

					tx.send(Event::TorrentEvent(
						torrent.id,
						TorrentEvent::PieceEvent(piece, PieceEvent::Done)
					))
					.unwrap();
				});
			}
			PieceEvent::Done => {
				if session.torrents[&torrent_id].torrent.lock().await.is_done() {
					session
						.tx
						.send(Event::TorrentEvent(torrent_id, TorrentEvent::Done))
						.unwrap();
				}
			}
		},
		TorrentEvent::Done => {
			log::info!("{prefix}");

			session.torrents[&torrent_id].complete().await;
		}
		_ => ()
	}
}
