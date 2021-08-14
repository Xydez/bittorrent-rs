use bittorrent::metainfo::MetaInfo;

fn main() {
	let meta = MetaInfo::load("debian-10.10.0-amd64-DVD-1.iso.torrent").unwrap();

	println!("{:?}", meta);
}
