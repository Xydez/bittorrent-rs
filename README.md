# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html), prioritizing simplicity, reliability and correctness.

## Getting started
1. Create a [`Session`](core::session::Session)
2. Load the [`MetaInfo`](protocol::metainfo::MetaInfo) from a torrent file
3. Create a [`Store`](io::store::Store) to store the downloaded data
4. Add the meta info and store to the session with [`Session::add`](core::session::Session::add)
5. Start the session with [`Session::start`](core::session::Session::start)

### Example
```rust,no_run
use bittorrent::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (session, _) = Session::spawn();
    let meta_info = MetaInfo::load("sample.torrent").unwrap();

    let store = FileStore::new(
        "downloads",
        meta_info.piece_size,
        meta_info.files
            .iter()
            .map(|file| (file.length, file.path.clone()))
            .collect::<Vec<_>>()
    )
    .unwrap();

    session.add_torrent(Torrent::new(meta_info, store));

    // Will keep on running until shutdown is called on the session
    session.join().await;
}
```

## Information
### Terminology
* *peer* - BitTorrent instance that downloads and seeds torrents
* *client* - The locally running BitTorrent instance. The *client* is by definiton also a *peer*.
* *torrent* - Complete file or files as described in a metainfo file
* *piece* - Part of a torrent that is described in the metainfo file and can be verified by a SHA1 hash
* *block* - Segment of a piece that a *client* may request from a *peer*

### Reading material
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
* Descriptions of algorithms in the paper [Rarest First and Choke Algorithms Are Enough - Section 2.2](http://conferences.sigcomm.org/imc/2006/papers/p20-legout.pdf)

### Coding guidelines
* `use super::..` is forbidden outside test modules
* All code must be formatted with `rustfmt`
* Follow the guidelines for log levels
  * **Error** - Something has failed, but the application can keep on running
  * **Warn** - Something unexpected has occurred, and should probably be investigated sooner or later
  * **Info** - Information on important events within the application
  * **Debug** - Events useful to debugging issues with the application
  * **Trace** - Redundant fine-grained details showing the step-by-step execution of the program

## TODO
### Investigations
* Find a way to use `Sink` and `Stream` with `Wire`
  * Investigate [tokio::io::split](https://docs.rs/tokio/1.21.2/tokio/io/fn.split.html) to split read/write streams
  * Investigate [tokio_util::codec](https://docs.rs/tokio-util/0.6.10/tokio_util/codec/index.html)
* Investigate [tracing](https://lib.rs/crates/tracing) for better logging

### Optimization
* Platform optimized io
  * Unix: Use `pwritev` in the [*nix](https://lib.rs/crates/nix) crate
    * The difference between `write` and `writev` is that `writev` writes multiple buffers into one contiguous slice in the file, which removes the need to copy to a new buffer before writing
    * The difference between `write` and `pwrite` is that `pwrite` specifies the offset, which means `seek` does not need to be called, halving the amount of system calls
  * Windows: Use [`WriteFile`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile)
    * See the [`lpOverlapped`](https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-overlapped) parameter
* Maybe use [dashmap](https://lib.rs/crates/dashmap) for better performance
* Maybe use [hashbrown](https://lib.rs/crates/hashbrown) for better performance
* Check if we should `Weak` instead of `Arc` for some things that should not be kept alive
* Look for ways to optimize away some mutexes/rwlocks
* Use `Bytes` where applicable

### Features
* Magnet links
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
* Allow setting torrent modes
* Document all the code (`#![warn(missing_docs)]`)
  * Follow the [documentation guidelines](https://rust-lang.github.io/api-guidelines/documentation.html)

### Changes
* Standardize more of the code
  * Piece ID and piece size
  * Change all incorrect instances of *length* into *size*
* It might be a good idea to let multiple peers work on the same piece normally
  * We could change the PieceIterator to no longer have a "current download" and just check the ongoing `torrent.downloads` before calling the picker.
  * This might even mean we could get rid of the PieceIterator which looks like an ugly workaround anyways
* Rename maybe_blocks in worker to "out_of_blocks" or something which is clearer
* Don't connect to all peers received in Announce (See inofficial spec)
  * Follow the recommendations
    * Only actively form connections if client has less than 30 peers
    * Refuse connections after client has a maximum of 55 peers
  * PeerConnected messages
* Make a lot of things pub(crate) instead of pub
* Properly manage the tasks of peer workers
  * We can join the workers in the event loop
* `pretty-assertions` dev dependency

### Notes
* Make sure all Worker in session.peers are alive
  * Remember to join the tasks
* Make sure piece.availability is updated
* Send `bittorrent::wire::Message::Cancel` if session shuts down during download
* Announce started/completed/stopped to tracker
* Update `piece.availability` when bitfield/have is received
* Proper fix for endgame
  * The principle is: when all blocks are downloading the remaining peers may download already downloading blocks
* Find a way to receive when a block has been cancelled
  * I think passing a broadcast to all `get_block` instances is the best way to do this

### Problems
* Fix this warning
  * `WARN [bittorrent::core::session] download for piece 1291 block 15 not found, block downloaded in vain`
  * Maybe this occurrs if two peers are downloading the same block and one of them sets the block to pending, or something? Probably scratch that but..
* Fix sometimes getting stuck near end
  * `INFO [bittorrent_cli]    0 PENDING   3 DOWNLOADING   0 VERIFYING  1330 DONE   0 OTHER`
