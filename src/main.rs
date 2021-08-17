use bittorrent::{metainfo::MetaInfo, session::Session, tracker::{Announce, Tracker}};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let mut session = Session::new();

	let meta = MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();
	println!("== {} ==", meta.name);
	println!("{:<16}{}", "tracker", meta.tracker);
	print!("{:<16}", "info hash");
	for b in meta.info_hash {
		print!("{:x}", b);
	}
	println!();

	let tracker = Tracker::new(&meta.tracker);

	let response = tracker.announce(&Announce {
		info_hash: meta.info_hash,
		peer_id: ['x'; 20],
		ip: None,
		port: 8000,
		uploaded: 0,
		downloaded: 0,
		left: 0,
		event: None
	}).await.unwrap();

	session.add(meta);

	println!("{:#?}", response);
}
