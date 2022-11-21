use argh::FromArgs;
use bittorrent::{
	core::{
		event::{Event, PieceEvent, TorrentEvent},
		piece::State
	},
	prelude::*
};

//const TORRENT: &str = "torrents/[SubsPlease] Kage no Jitsuryokusha ni Naritakute! - 06 (1080p) [9E88E130].mkv.torrent";
//const DOWNLOAD_DIR: &str = "downloads";

/// Download a torrent
#[derive(FromArgs, Debug)]
struct Args {
	/// file name or Magnet URI of the torrent
	#[argh(positional)]
	torrent: String,

	/// download directory, defaults to the current dir
	#[argh(option, default = "\"downloads\".to_string()")]
	dir: String
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let _logger = flexi_logger::Logger::try_with_str(
		"debug, bittorrent=trace, bittorrent::core::worker=debug"
	)
	.unwrap()
	.log_to_file(flexi_logger::FileSpec::default().directory("logs"))
	.write_mode(flexi_logger::WriteMode::BufferDontFlush)
	.print_message()
	.duplicate_to_stderr(flexi_logger::Duplicate::Info)
	.format_for_files(flexi_logger::detailed_format)
	.start()
	.unwrap();

	let args: Args = argh::from_env();

	let mut session = Session::new();

	let meta_info = MetaInfo::load(&args.torrent).unwrap();
	std::fs::create_dir_all(args.dir.clone()).unwrap();
	let store = FileStore::new(
		meta_info.piece_size,
		meta_info
			.files
			.iter()
			.map(|file| {
				(
					file.length,
					std::path::Path::new(&args.dir).join(&file.path)
				)
			})
			.collect::<Vec<_>>()
	)
	.unwrap();

	session.add(meta_info, Box::new(store));

	session.add_listener(|session: &Session, event: &Event| match event {
		Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(_piece, PieceEvent::Done)) => {
			let torrent = torrent.clone();

			tokio::spawn(async move {
				let (pending, downloading, verifying, done, other) = {
					let torrent = torrent.read().await;

					let (mut pending, mut downloading, mut verifying, mut done, mut other) =
						(0, 0, 0, 0, 0);

					for piece in torrent.pieces.iter() {
						match piece.state {
							State::Pending => pending += 1,
							State::Downloading => downloading += 1,
							State::Verifying => verifying += 1,
							State::Done => done += 1,
							State::Ignore => (),
							_ => other += 1
						}
					}

					(pending, downloading, verifying, done, other)
				};

				log::info!(
					"{:>4} PENDING  {:>2} DOWNLOADING  {:>2} VERIFYING  {:>4} DONE  {:>2} OTHER",
					pending,
					downloading,
					verifying,
					done,
					other
				);
			});
		},
		Event::TorrentEvent(_torrent, TorrentEvent::Done) => session.stop(),
		_ => ()
	});

	session.start().await;
}
