use bittorrent::{metainfo::MetaInfo, session::Session};

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

	session.add(meta.clone()).await;

	loop {
		session.poll_events().await;
	}
}
