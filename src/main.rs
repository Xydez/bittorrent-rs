use std::fs::OpenOptions;

use bittorrent::{metainfo::MetaInfo, session::Session, store::SingleFileStore};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let mut session = Session::new(['x' as u8; 20]);

	let meta = MetaInfo::load("[Ohys-Raws] Slow Loop - 01 (AT-X 1280x720 x264 AAC JP).mp4.torrent")
		.unwrap();
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
				.open("[Ohys-Raws] Slow Loop - 01 (AT-X 1280x720 x264 AAC JP).mp4")
				.unwrap(),
			size
		)
		.unwrap()
	);

	session.add(meta.clone(), store).await;

	loop {
		session.poll_events().await;
	}
}
