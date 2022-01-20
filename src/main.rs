use std::fs::OpenOptions;

use bittorrent::{metainfo::MetaInfo, session::Session, store::SingleFileStore};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let mut session = Session::new(['x' as u8; 20]);

	let meta = MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();
	println!("== {} ==", meta.name);
	println!("{:<16}{}", "tracker", meta.announce);
	println!("{:<16}{}", "pieces", meta.pieces.len());
	print!("{:<16}", "info hash");
	for b in meta.info_hash {
		print!("{:x}", b);
	}
	println!();

	let size = meta.files.iter().fold(0, |acc, file| acc + file.length);

	// let store = Box::new(MemoryStore::new(size));
	let store = Box::new(
		SingleFileStore::new(
			OpenOptions::new()
				.read(true)
				.write(true)
				.create(true)
				.open("download/debian-10.10.0-amd64-DVD-1.iso")
				.unwrap(),
			size
		)
		.unwrap()
	);

	let torrent = session.add(meta.clone(), store).await;

	// Exit when the torrent is done.
	while !torrent.lock().await.done() {
		session.poll_events().await;
	}

	println!("Torrent has finished downloading.");
}
