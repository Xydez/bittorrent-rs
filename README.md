# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

## Reading material
* [Inofficial BitTorrent specification](https://wiki.theory.org/index.php/BitTorrentSpecification)
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

## TODO
### Investigations
* Investigate [tokio::io::split](https://docs.rs/tokio/1.21.2/tokio/io/fn.split.html) to split read/write streams
* Investigate [tracing](https://lib.rs/crates/tracing) for better logging

### Features
* Generic `StoreWriter` to write to the store more efficiently (buffered writes / write_vectored?)
  * Unix: Use `pwritev` in [*nix](https://lib.rs/crates/nix)
* Magnet links
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces

### Changes
* ~~We should wait until a torrent added message or something like that in the peer threads instead of sleeping~~
* Find a nicer way to handle bytes, such as a trait to convert to/from bytes, as well as using `Bytes` instead of `Vec<u8>`?
* Write and use [peer_id](src/protocol/peer_id.rs)
* Standardize more of the code: Piece index is always a `u32`, piece size is always a `usize`
* **Implement std::fmt::Display for message so it prints nicer in the console - currently prints large chunks of binary data**
* **Use [thiserror](https://lib.rs/crates/thiserror) to write better errors**

### Notes
* Make sure all Worker in session.peers are alive
* Make sure piece.availability is updated

## Active projects
### Rewrite peer_worker
We want to weave all requests in one, so basically we have a thread that loops and receives all requests for a peer and uses broadcast to dispatch them to the right task. Using select we also receive commands which tell us to choke / unchoke the peer. So basically one big peer worker with a task that selects everything and handles it, THIS INCLUDES CHECKING peer_interested AND SEEDING, FINALLY.

* Descriptions of algorithms in the paper [Rarest First and Choke Algorithms Are Enough - Section 2.2](http://conferences.sigcomm.org/imc/2006/papers/p20-legout.pdf)
* Track and download individual blocks instead of pieces
  * We have a peer worker where we spawn block download tasks. Each task has a broadcast receiver of messages received from the peer and a transmitter of blocks received.

### Current state
Okay so basically `Block` is done and now we need to do:
* Rewrite peer_worker
  * Update `piece.availability` when bitfield/have is received
  * ~~Use [algorithm](src/core/algorithm.rs) to select pieces~~
  * Create a `PieceEvent::Block` or something and assemble the piece in [session](src/core/session.rs) (?)
  * ~~Note: Currently nothing happens after permit acquired - suspecting a deadlock~~
    * ~~Log case 1 goes "loop iteration" -> "permit acquired"~~
    * ~~Log case 2 goes "loop iteration" -> bitfield -> SendError(Bitfield(...))~~
