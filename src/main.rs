use bittorrent::{metainfo::MetaInfo, session::Session};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let mut session = Session::new(['x' as u8; 20]);
	
	let meta = MetaInfo::load("[Ohys-Raws] Kimetsu no Yaiba Yuukaku Hen - 04 (CX 1280x720 264 AAC).mp4.torrent").unwrap();
	// let meta = MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();
	println!("== {} ==", meta.name);
	println!("{:<16}{}", "tracker", meta.announce);
	println!("{:<16}{}", "pieces", meta.pieces.len());
	print!("{:<16}", "info hash");
	for b in meta.info_hash {
		print!("{:x}", b);
	}
	println!();

	// let tracker = Tracker::new(&meta.announce);

	// let response = tracker.announce(&Announce {
	// 	info_hash: meta.info_hash,
	// 	peer_id: PEER_ID,
	// 	ip: None,
	// 	port: 8000,
	// 	uploaded: 0,
	// 	downloaded: 0,
	// 	left: 0,
	// 	event: None
	// }).await.unwrap();

	// println!("{:#?}", response);

	// SocketAddrV4::from_str(...).unwrap()
	// let peer = Peer::connect(response.peers_addrs[0], &meta, &PEER_ID).await.unwrap();
	// println!("{:#?}", peer);

	session.add(meta.clone()).await;

	loop {
		// println!("poll_events");
		session.poll_events().await;
		// tokio::time::sleep(Duration::from_secs_f64(0.5)).await;
	}
}
