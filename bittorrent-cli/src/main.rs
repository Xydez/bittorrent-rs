use std::path::PathBuf;

use argh::FromArgs;
use bittorrent::{
	core::{
		event::{
			Event,
			PieceEvent,
			TorrentEvent
		},
		piece::State
	},
	prelude::*
};

/// Download a torrent
#[derive(FromArgs, Debug)]
struct Args {
	/// file name or Magnet URI of the torrent
	#[argh(positional)]
	torrent: String,

	/// should the client skip reading and writing resume data
	#[argh(switch)]
	skip_resume: bool,

	/// download directory, defaults to the current dir
	#[argh(option, default = "\"downloads\".into()")]
	dir: PathBuf
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

	let (session, mut rx) = Session::spawn();

	let resume_file = std::path::Path::new(&args.torrent).with_extension("resume");

	// Attempt to resume a previous instance
	let torrent = match if args.skip_resume {
		None
	} else {
		Some(std::fs::File::open(&resume_file).map(|file| {
			Torrent::deserialize_from(file, |resume_data| {
				FileStore::resume(&args.dir, resume_data).unwrap()
			})
		}))
	} {
		Some(Ok(Ok(value))) => {
			log::info!(
				"Resume data was loaded, {}/{} pieces done",
				value.complete_pieces().count(),
				value.pieces.len()
			);
			value
		}
		None | Some(Err(_)) | Some(Ok(Err(_))) => {
			if !args.skip_resume {
				log::info!("No resume data could be loaded");
			}

			let meta_info = MetaInfo::load(&args.torrent).unwrap();
			std::fs::create_dir_all(args.dir.clone()).unwrap();
			let store = FileStore::from_meta_info(&args.dir, &meta_info).unwrap();

			Torrent::new(meta_info, store)
		}
	};

	// TODO: We can remove the need for async if we convert it to TorrentPtr in SessionHandle
	let torrent = session.add_torrent(torrent).await;

	{
		let torrent = torrent.clone();
		let resume_file = resume_file.clone();

		ctrlc::set_handler(move || {
			log::info!("CTRL+C pressed");

			if !args.skip_resume {
				log::info!("Writing resume data...");
				let mut file = std::fs::File::create(&resume_file).unwrap();
				torrent.blocking_read().serialize_into(&mut file).unwrap();
			}

			std::process::exit(0);
		})
		.unwrap();
	}

	loop {
		let event = match rx.recv().await {
			Ok(event) => event,
			Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
				log::warn!("Lagged {count} messaged behind");
				continue;
			}
			Err(tokio::sync::broadcast::error::RecvError::Closed) => {
				log::info!("Receiver is closed, terminating loop");
				break;
			}
		};

		match event {
			Event::TorrentEvent(_, TorrentEvent::PieceEvent(_piece, PieceEvent::Done)) => {
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
			}
			Event::TorrentEvent(_, TorrentEvent::Done) => session.shutdown(),
			_ => ()
		}
	}

	session.join().await;

	if !args.skip_resume {
		log::info!("Writing resume data...");
		let mut file = std::fs::File::create(&resume_file).unwrap();
		tokio::task::spawn_blocking(move || {
			torrent.blocking_read().serialize_into(&mut file).unwrap()
		})
		.await
		.unwrap();
	}
}
