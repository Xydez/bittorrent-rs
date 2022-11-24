# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

## Terminology
* *peer* - BitTorrent instance that downloads and seeds torrents
* *client* - The locally running BitTorrent instance. The *client* is by definiton also a *peer*.
* *torrent* - Complete file or files as described in a metainfo file
* *piece* - Part of a torrent that is described in the metainfo file and can be verified by a SHA1 hash
* *block* - Segment of a piece that a *client* may request from a *peer*

## Reading material
* [Inofficial BitTorrent specification](https://wiki.theory.org/BitTorrentSpecification)
* https://blog.jse.li/posts/torrent/
* https://en.wikipedia.org/wiki/Torrent_file
* https://www.bittorrent.org/beps/bep_0003.html
* http://www.kristenwidman.com/blog/71/how-to-write-a-bittorrent-client-part-2/
* https://wiki.theory.org/BitTorrentSpecification#cancel:_.3Clen.3D0013.3E.3Cid.3D8.3E.3Cindex.3E.3Cbegin.3E.3Clength.3E
* https://blog.webtor.io/en/post/technologies-inside-webtor.io/
* https://docs.fileformat.com/video/mkv/
* https://tools.ietf.org/id/draft-ietf-cellar-ebml-03.html#rfc.section.1
* https://github.com/webtor-io/content-transcoder
* https://www.bittorrent.org/beps/bep_0009.html
* https://www.bittorrent.org/beps/bep_0010.html
* http://conferences.sigcomm.org/imc/2006/papers/p20-legout.pdf

## TODO
### Investigations
* Find a way to use `Sink` and `Stream` with `Wire`
  * Investigate [tokio::io::split](https://docs.rs/tokio/1.21.2/tokio/io/fn.split.html) to split read/write streams
  * Investigate [tokio_util::codec](https://docs.rs/tokio-util/0.6.10/tokio_util/codec/index.html)
* Investigate [tracing](https://lib.rs/crates/tracing) for better logging
* Investigate using cargo-audit and cargo-deny to use secure libraries with correct licenses (see [rustsec](https://rustsec.org/))
* We could use [dashmap](https://lib.rs/crates/dashmap) for better performance
  * Create an optimization heading?

### Features
* Generic `StoreWriter` to write to the store more efficiently (buffered writes / write_vectored?)
  * Unix: Use `pwritev` in [*nix](https://lib.rs/crates/nix)
* Magnet links
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
* Write and use [peer_id](src/protocol/peer_id.rs)
* Resuming downloads
  * Keep resume data in a file beside the torrent (<torrent_name>.resume)
* Document all the code

### Changes
* Create a `verification_worker.rs` that handles verifying pieces
* Standardize more of the code
  * Piece ID and piece size
  * Change all incorrect instances of *length* into *size*
* Look for ways to optimize away some mutexes/rwlocks
* Find a nicer way to handle bytes, such as a trait to convert to/from bytes, as well as using `Bytes` instead of `Vec<u8>`?
  * The current way works just fine, though?
* Use [tokio::task::JoinSet](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html) in Session to track the peer tasks
* It doesn't make sense for the `Worker` struct to be defined in `worker.rs` but used only in `session.rs`
* It might be a good idea to let multiple peers work on the same piece normally
  * We could change the PieceIterator to no longer have a "current download" and just check the ongoing `torrent.downloads` before calling the picker.
  * This might even mean we could get rid of the PieceIterator which looks like an ugly workaround anyways
* Rename maybe_blocks in worker to "out_of_blocks" or something which is clearer
* Don't connect to all peers received in Announce (See inofficial spec)
  * Only actively form connections if client has less than 30 peers
  * Refuse connections after client has a maximum of 55 peers
  * PeerConnected messages
* Use `Weak` instead of `Arc` for things that should not be kept alive
* Make a lot of things pub(crate) instead of pub

### Notes
* Make sure all Worker in session.peers are alive
  * Remember to join the tasks
* Make sure piece.availability is updated
* Send `bittorrent::wire::Message::Cancel` if session shuts down during download
* Announce started/completed/stopped to tracker

## Active projects
### Rewrite peer_worker
We want to weave all requests in one, so basically we have a thread that loops and receives all requests for a peer and uses broadcast to dispatch them to the right task. Using select we also receive commands which tell us to choke / unchoke the peer. So basically one big peer worker with a task that selects everything and handles it, THIS INCLUDES CHECKING peer_interested AND SEEDING, FINALLY.

* Descriptions of algorithms in the paper [Rarest First and Choke Algorithms Are Enough - Section 2.2](http://conferences.sigcomm.org/imc/2006/papers/p20-legout.pdf)
* Track and download individual blocks instead of pieces
  * We have a peer worker where we spawn block download tasks. Each task has a broadcast receiver of messages received from the peer and a transmitter of blocks received.

### Current state
* Update `piece.availability` when bitfield/have is received
* Enable endgame when all pieces are downloading
* Find a way to receive when a block has been cancelled
  * I think passing a broadcast to all `get_block` instances is the best way to do this
* **Find a way to manage the peer threads in Session**
  * Need to join the workers in the event loop

### Errors
* Fix this warning
  * `WARN [bittorrent::core::session] download for piece 1291 block 15 not found, block downloaded in vain`
