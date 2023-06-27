use std::{net::Ipv4Addr, sync::Arc};

use common::util;
use log::{debug, error, trace};
use protocol::wire::{
	self,
	connection::{Handshake, Wire}
};
use tokio::{
	net::{TcpListener, TcpStream},
	sync::Mutex
};

use super::{EventReceiver, Session};
use crate::{
	core::event::Event,
	peer::Peer,
	session::handler,
	torrent::{Torrent, TorrentHandle, TorrentId}
};

#[derive(Debug)]
pub enum Command {
	AddTorrent(tokio::sync::oneshot::Sender<TorrentId>, Torrent),
	Shutdown
}

pub type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
pub type CommandReceiver = tokio::sync::mpsc::UnboundedReceiver<Command>;

/// Starts the event loop of the session
///
/// The event loop keeps on running until [Event::Stopped] is received
pub async fn run(
	session: Arc<Mutex<Session>>,
	mut cmd_rx: CommandReceiver,
	mut event_rx: EventReceiver
) {
	let config = session.lock().await.config.clone();

	// TCP socket server, listens for connections from peers
	let listener = TcpListener::bind((Ipv4Addr::new(127, 0, 0, 1), config.port))
		.await
		.unwrap();

	session.lock().await.tx.send(Event::Started).unwrap();

	loop {
		tokio::select! {
			Some(event) = event_rx.recv() => {
				let mut session = session.lock().await;
				let should_close = matches!(event, Event::Stopped);

				handler::handle(&mut session, &event).await;
				session.broadcast_tx.send(event).unwrap();

				// Stop the event loop if the session has stopped
				if should_close {
					break;
				}
			},
			command = cmd_rx.recv() => {
				if let Some(command) = command {
					let mut session = session.lock().await;

					match command {
						Command::AddTorrent(sender, torrent) => {
							sender.send(session.add_torrent(torrent).await).unwrap();
						},
						Command::Shutdown => session.tx.send(Event::Stopped).unwrap()
					}
				} else {
					break;
				}
			},
			result = listener.accept() => {
				match result {
					Ok((stream, addr)) => {
						trace!("Incoming connection accepted with address {addr}");

						match handle_connection(&*session.lock().await, stream).await {
							Ok((peer, torrent)) => {
								torrent.add_conn(peer);
							}
							Err(error) => {
								error!("Failed to perform handshake with address {addr}: {}", util::error_chain(error));
							}
						}
					},
					Err(error) => {
						error!("Failed to accept incoming connection: {}", util::error_chain(error));
					}
				}
			}
			// TODO: Join the torrents when we have a JoinMap
		}
	}

	debug!("Session is shutting down");
	//self.shutdown().await;
}

async fn handle_connection(
	session: &Session,
	stream: TcpStream
) -> wire::connection::Result<(Peer, Arc<TorrentHandle>)> {
	// Create wire & send on conn_tx
	let mut wire = Wire::new(stream);

	// TODO: If the initiator of the connection receives a handshake in which
	//       the peer_id does not match the expected peer_id, then the initiator
	//       is expected to drop the connection. Note that the initiator
	//       presumably received the peer information from the tracker, which
	//   	 includes the peer_id that was registered by the peer. The peer_id
	//       from the tracker and in the handshake are expected to match.

	let handshake = wire.receive_handshake().await?;

	let torrent_handle = session
		.torrents
		.values()
		.find(|torrent_handle| torrent_handle.torrent.meta_info.info_hash == handshake.info_hash)
		.ok_or(wire::connection::Error::InvalidHandshake)?
		.clone();

	wire.send_handshake(&Handshake::new(
		torrent_handle.torrent.meta_info.info_hash,
		session.config.peer_id,
		session.config.extensions
	))
	.await?;

	let peer = Peer::new(wire, handshake);

	Ok((peer, torrent_handle))
}
