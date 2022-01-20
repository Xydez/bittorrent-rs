use std::{collections::VecDeque, sync::Arc, time::Duration};

use tokio::sync::{mpsc::Sender, Mutex};

use crate::{
	peer::{Peer, PeerError},
	session::{Event, State, TorrentPtr, Work},
	wire::Message
};

/// An increased block size means an increase in performance, but doesn't work on many clients. 16 KiB is the default.
const BLOCK_SIZE: usize = 16_384;

/// Sever the connection if the peer doesn't send a message within this duration. This amount of time is generally 2 minutes.
const ALIVE_TIMEOUT: Duration = Duration::from_secs_f64(120.0);

/// The maximum amount of simultaneous requests that can be sent to a peer
const MAX_REQUESTS: usize = 5;

enum DownloadError {
	Timeout,
	Choked,
	MessageError,
	PeerError(PeerError)
}

impl From<tokio::time::error::Elapsed> for DownloadError {
	fn from(_: tokio::time::error::Elapsed) -> Self {
		DownloadError::Timeout
	}
}

impl From<PeerError> for DownloadError {
	fn from(error: PeerError) -> Self {
		DownloadError::PeerError(error)
	}
}

pub(crate) async fn run_worker(
	mut peer: Peer,
	work_queue: Arc<Mutex<VecDeque<Work>>>,
	torrent: TorrentPtr,
	tx: Sender<Event>
) {
	peer.send(Message::Interested)
		.await
		.expect("Failed to send Message::Interested");

	loop {
		if peer.peer_choking() {
			// 1. Wait until we are unchoked
			peer.receive()
				.await
				.expect("Failed to receive message (while waiting for unchoke)");
		} else {
			// 2. As long as we are unchoked, do work from the queue

			let work = {
				let mut lock = work_queue.lock().await;

				if let Some(i) = lock.iter().position(|v| peer.has_piece(v.piece)) {
					Some(lock.remove(i).unwrap())
				} else {
					None
				}
			};

			if let Some(work) = work {
				torrent.lock().await.pieces[work.piece] = State::Downloaing;

				println!(
					"Downloading piece {} [{}]",
					work.piece,
					crate::util::to_hex(peer.peer_id())
				);

				match get_piece(&mut peer, &work).await {
					Ok(data) => {
						torrent.lock().await.pieces[work.piece] = State::Downloaded;
						tx.send(Event::PieceReceived(work, data)).await.unwrap();
					},
					Err(err) => {
						println!("Failed to download piece {}", work.piece);

						torrent.lock().await.pieces[work.piece] = State::Pending;
						work_queue.lock().await.push_back(work);

						match err {
							DownloadError::Timeout => {
								eprintln!("Connection timed out");
								break;
							},
							DownloadError::PeerError(error) => {
								eprintln!("A peer error occurred: {:#?}", error);
								break;
							},
							_ => ()
						}
					}
				}
			} else if torrent
				.lock()
				.await
				.pieces
				.iter()
				.all(|v| v == &State::Done)
			{
				// All work is completed and the peer worker should quit.
				break;
			} else {
				// There is no work currently able to be downloaded by this peer.
				// TODO: If it's because there *is* no work, wait 5 seconds. If it's because the peer doesn't have a piece, wait for messages from the peer.
				tokio::time::sleep(Duration::from_secs_f64(5.0)).await;
			}
		}
	}

	println!("Severing connection");
}

// TODO: We might want to stop "counting requests" and use a channel instead. Maybe start another "listening thread" and use a channel to .recv the pieces (incoming pieces only channel, basically)
async fn get_piece(peer: &mut Peer, work: &Work) -> Result<Vec<u8>, DownloadError> {
	let mut blocks = Vec::new();

	let mut active_requests = 0;

	// TODO: Make this code cleaner. Right now we have two blocks of identical piece receiving code.

	for i in (0..work.length).step_by(BLOCK_SIZE) {
		let block_size = (work.length - i).min(BLOCK_SIZE);

		// println!("Requesting piece {} block {}", work.piece, i);
		tokio::time::timeout(
			ALIVE_TIMEOUT,
			peer.send(Message::Request(
				work.piece as u32,
				i as u32,
				block_size as u32
			))
		)
		.await??;

		active_requests += 1;

		while active_requests >= MAX_REQUESTS {
			match tokio::time::timeout(ALIVE_TIMEOUT, peer.receive()).await?? {
				Message::Piece(_index, begin, block) => {
					// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
					blocks.push((begin, block));
					active_requests -= 1;
				},
				Message::Choke => {
					return Err(DownloadError::Choked);
				},
				Message::KeepAlive | Message::Unchoke => (),
				msg => {
					// TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
					eprintln!("Unexpected message (while receiving piece): {:?}", msg);
					return Err(DownloadError::MessageError);
				}
			}
		}
	}

	while active_requests > 0 {
		match tokio::time::timeout(ALIVE_TIMEOUT, peer.receive()).await?? {
			Message::Piece(_index, begin, block) => {
				// println!("Received piece {} block {} ({:.1}%)", index, begin, 100.0 * (begin as f64 + block.len() as f64) / work.length as f64);
				blocks.push((begin, block));
				active_requests -= 1;
			},
			Message::Choke => {
				return Err(DownloadError::Choked);
			},
			Message::KeepAlive | Message::Unchoke => (),
			msg => {
				// TODO: Maybe we should panic instead. If we receive an unexpected message, the peer is likely to be a retard.
				eprintln!("Unexpected message (while receiving piece): {:?}", msg);
				return Err(DownloadError::MessageError);
			}
		}
	}

	// Assemble the blocks into a piece
	println!("Assembling piece {}", work.piece);
	blocks.sort_by(|(a, _), (b, _)| a.cmp(&b));
	let data = blocks
		.into_iter()
		.map(|(_offset, data)| data)
		.flatten()
		.collect::<Vec<_>>();

	Ok(data)
}
