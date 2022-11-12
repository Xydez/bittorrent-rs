use bittorrent::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        piece::State,
        session::Session,
    },
    io::store::FileStore,
    protocol::metainfo::MetaInfo,
};

const TORRENT: &str = "torrents/debian-10.10.0-amd64-DVD-1.iso.torrent";
//const TORRENT: &str = "torrents/[SubsPlease] Yofukashi no Uta - 12 (1080p) [6529938D].mkv.torrent";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    pretty_env_logger::init();

    let mut session = Session::new([b'x'; 20]);

    session.add_listener(|session: &Session, event: &Event| {
        // log::info!("[event] {:#?}", event.as_ref());

        #[allow(clippy::single_match)]
        match event {
            Event::TorrentEvent(torrent, TorrentEvent::PieceEvent(_piece, PieceEvent::Done)) => {
                let torrent = torrent.clone();

                tokio::spawn(async move {
                    let (pending, downloading, done) = {
                        let torrent = torrent.lock().await;

                        let (mut pending, mut downloading, mut done) = (0, 0, 0);

                        for piece in torrent.pieces.iter() {
                            match piece.state {
                                State::Pending => pending += 1,
                                State::Downloading
                                | State::Downloaded
                                | State::Verifying
                                | State::Verified => downloading += 1,
                                State::Done => done += 1,
                                State::Ignore => (),
                            }
                        }

                        (pending, downloading, done)
                    };

                    log::info!(
                        "{:>4} PENDING | {:>4} DOWNLOADING | {:>4} DONE",
                        pending,
                        downloading,
                        done
                    );
                });
            }
            Event::TorrentEvent(_torrent, TorrentEvent::Done) => session.shutdown(),
            _ => (),
        }
    });

    let meta_info = MetaInfo::load(TORRENT).unwrap();

    std::fs::create_dir_all("./downloads/").unwrap();
    let store = FileStore::new(
        meta_info.piece_size,
        meta_info.files
            .iter()
            .map(|file| (file.length, std::path::Path::new("./downloads/").join(&file.path)))
            .collect::<Vec<_>>()
    )
    .unwrap();

    session.add(meta_info, Box::new(store));

    session.start().await;
}
