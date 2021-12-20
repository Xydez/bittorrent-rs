use std::time::Duration;

use bittorrent::{metainfo::MetaInfo, tracker::{Announce, Tracker}, session::Session};

const PEER_ID: [u8; 20] = ['x' as u8; 20];

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let mut session = Session::new(['x' as u8; 20]);

	let meta = MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();
	println!("== {} ==", meta.name);
	println!("{:<16}{}", "tracker", meta.announce);
	print!("{:<16}", "info hash");
	for b in meta.info_hash {
		print!("{:x}", b);
	}
	println!();

	let tracker = Tracker::new(&meta.announce);

	let response = tracker.announce(&Announce {
		info_hash: meta.info_hash,
		peer_id: PEER_ID,
		ip: None,
		port: 8000,
		uploaded: 0,
		downloaded: 0,
		left: 0,
		event: None
	}).await.unwrap();

	session.add(meta.clone()).await;

	println!("{:#?}", response);

	// SocketAddrV4::from_str(...).unwrap()
	// let peer = Peer::connect(response.peers_addrs[0], &meta, &PEER_ID).await.unwrap();
	// println!("{:#?}", peer);

	loop {
		tokio::time::sleep(Duration::from_secs_f64(0.5)).await;
	}
}
