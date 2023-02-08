use std::{path::PathBuf, sync::Arc};

use argh::FromArgs;
use bittorrent::{
	core::{
		event::{Event, PieceEvent, TorrentEvent},
		piece::State,
		util
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

#[tokio::main] // (flavor = "current_thread")
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
	let session = Arc::new(session);

	/*
	std::fs::create_dir_all(args.dir.clone()).unwrap();
	FileStore::
	let store = FileStore::from_meta_info(&args.dir, &meta_info).unwrap();



	Torrent::new(meta_info, store)
	Torrent::new_resumed(resume, resume_data)
	*/

	let resume_file = std::path::Path::new(&args.torrent).with_extension("resume");
	let resume_data = (!args.skip_resume && resume_file.exists())
		.then(|| bincode::deserialize_from(std::fs::File::open(&resume_file).unwrap()).unwrap());

	let torrent = match resume_data {
		Some(resume_data) => {
			Torrent::new_resumed(FileStore::resume(args.dir), resume_data).unwrap()
		}
		None => {
			let meta_info = MetaInfo::load(&args.torrent).unwrap();
			let store = FileStore::from_meta_info(args.dir, &meta_info).unwrap();

			Torrent::new(meta_info, store)
		}
	};

	/*
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
	*/

	// TODO: We can remove the need for async if we convert it to TorrentPtr in SessionHandle
	let torrent_id = session.add_torrent(torrent).await;

	{
		let session = session.clone();
		let resume_file = resume_file.clone();

		ctrlc::set_handler(move || {
			log::info!("CTRL+C pressed");

			if !args.skip_resume {
				log::info!("Writing resume data...");

				let session = session.clone();

				// TODO: This crashes because it's being called outside a tokio runtime, do we create a new runtime or something?
				let resume_data = tokio::runtime::Handle::current()
					.block_on(async move { session.torrent(torrent_id).await.resume_data().await });

				if let Err(error) = bincode::serialize_into(
					std::fs::File::create(&resume_file).unwrap(),
					&resume_data
				) {
					log::error!("Failed to write resume data: {}", util::error_chain(error));
				}
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
				let (pending, verifying, writing, done) = {
					let torrent = session.torrent(torrent_id).await;

					let (mut pending, mut verifying, mut writing, mut done) =
						(0, 0, 0, 0);

					for piece in torrent.lock().await.state().pieces.iter() {
						match piece.state {
							State::Pending => pending += 1,
							State::Verifying => verifying += 1,
							State::Writing => writing += 1,
							State::Done => done += 1,
							State::Ignore => ()
						}
					}

					(pending, verifying, writing, done)
				};

				log::info!(
					"{:>4} PENDING -> {:>2} VERIFYING -> {:>2} WRITING -> {:>4} DONE",
					pending,
					verifying,
					writing,
					done,
				);
			}
			Event::TorrentEvent(_, TorrentEvent::Done) => session.shutdown(),
			_ => ()
		}
	}

	if !args.skip_resume {
		log::info!("Writing resume data...");
		let resume_data = session.torrent(torrent_id).await.resume_data().await;

		if let Err(error) = tokio::task::spawn_blocking(move || {
			bincode::serialize_into(std::fs::File::create(&resume_file).unwrap(), &resume_data)
		})
		.await
		.unwrap()
		{
			log::error!("Failed to write resume data: {}", util::error_chain(error));
		}
	}

	session.join().await;
}
