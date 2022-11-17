use bittorrent::{
	core::{
		event::{Event, PieceEvent, TorrentEvent},
		piece::State
	},
	prelude::*
};

//const TORRENT: &str = "torrents/debian-10.10.0-amd64-DVD-1.iso.torrent";
const TORRENT: &str = "torrents/[SubsPlease] Kage no Jitsuryokusha ni Naritakute! - 06 (1080p) [9E88E130].mkv.torrent";
const DOWNLOAD_DIR: &str = "downloads";

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

	let mut session = Session::new();

	session.add_listener(|session: &Session, event: &Event| {
        // log::info!("[event] {:#?}", event.as_ref());

        #[allow(clippy::single_match)]
        match event {
            Event::Started => {
                let meta_info = MetaInfo::load(TORRENT).unwrap();
                std::fs::create_dir_all(DOWNLOAD_DIR).unwrap();
                let store = FileStore::new(
                    meta_info.piece_size,
                    meta_info.files
                        .iter()
                        .map(|file| (file.length, std::path::Path::new(DOWNLOAD_DIR).join(&file.path)))
                        .collect::<Vec<_>>()
                )
                .unwrap();

                session.add(meta_info, Box::new(store));
            },
            Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(_piece, PieceEvent::Done)) => {
                let torrent = torrent.clone();

                tokio::spawn(async move {
                    let (pending, downloading, verifying, done, other) = {
                        let torrent = torrent.read().await;

                        let (mut pending, mut downloading, mut verifying, mut done, mut other) = (0, 0, 0, 0, 0);

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
            }
            Event::TorrentEvent(_torrent, TorrentEvent::Done) => session.shutdown(),
            _ => (),
        }
    });

	session.start().await;
}
