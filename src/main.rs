use bittorrent::{
    core::{
        event::{Event, PieceEvent, TorrentEvent},
        piece::State,
        session::Session,
    },
    io::store::NullStore,
    protocol::metainfo::MetaInfo,
};

const TORRENT: &str = "[SubsPlease] Yofukashi no Uta - 12 (1080p) [6529938D].mkv.torrent";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    pretty_env_logger::init();

    // TODO: Maybe we can swap out rx for a class with `pub(crate) rx: EventReceiver` so the user can't use rx
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

                    log::debug!(
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
    /*
    let file_info = meta_info.files.first().unwrap();
    let store = FileStore::new(
        File::create(format!(
            "./downloads/{}",
            file_info.path.as_os_str().to_str().unwrap()
        ))
        .unwrap(),
        file_info.length,
    )
    .unwrap();
    */
    let store = NullStore;

    session.add(meta_info, Box::new(store));

    session.start().await;
}
