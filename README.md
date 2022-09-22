# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

See:
* [Inofficial BitTorrent specification](https://wiki.theory.org/index.php/BitTorrentSpecification)
* See https://blog.jse.li/posts/torrent/
* See https://en.wikipedia.org/wiki/Torrent_file
* See https://www.bittorrent.org/beps/bep_0003.html
* See http://www.kristenwidman.com/blog/71/how-to-write-a-bittorrent-client-part-2/
* See https://wiki.theory.org/BitTorrentSpecification#cancel:_.3Clen.3D0013.3E.3Cid.3D8.3E.3Cindex.3E.3Cbegin.3E.3Clength.3E

## todo
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
  * Maybe have a priority byte for each torrent. The client will make sure all of the higher pieces are downloading before continuing to download the lower pieces.

-- Sidenote: live transcoding later --

* https://blog.webtor.io/en/post/technologies-inside-webtor.io/
* https://docs.fileformat.com/video/mkv/
* https://tools.ietf.org/id/draft-ietf-cellar-ebml-03.html#rfc.section.1
* https://github.com/webtor-io/content-transcoder

torrent to magnet:
session.getMagnet(magnet: string) -> MetaInfo

note:
* Should add trackers as well

see:
* https://www.bittorrent.org/beps/bep_0009.html
* https://www.bittorrent.org/beps/bep_0010.html

// TODO: We should wait until a torrent added message or something like that in the peer threads instead of sleeping

// TODO: Find a macro so we don't have to impl From<...> so many damn times

// TODO: Find a nice trait that converts objects to/from bytes. Maybe even use the standard serialize?

// TODO: Should we use &[u8]/Vec<u8> or Bytes?

TODO: Basically what we need to do is create a new `Peer` struct that can send and receive at the same time

// TODO: generic `PieceStrategy` to select pieces
// TODO: generic `StoreWriter` to write to the store more efficiently

// TODO: How do we weave sending and receiving messages to the peer effectively?
