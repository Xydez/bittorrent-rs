# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

See:
* [Inofficial BitTorrent specification](https://wiki.theory.org/index.php/BitTorrentSpecification)

## todo
* Use Weak ref instead of Arc in the peer threads to shut down correctly.
* Use two threads and a channel for receiving pieces instead of crashing if we receive a message we didn't expect. Also clean up code.
* See https://blog.jse.li/posts/torrent/
* See https://en.wikipedia.org/wiki/Torrent_file
* See https://www.bittorrent.org/beps/bep_0003.html
* See http://www.kristenwidman.com/blog/71/how-to-write-a-bittorrent-client-part-2/
* See https://wiki.theory.org/BitTorrentSpecification#cancel:_.3Clen.3D0013.3E.3Cid.3D8.3E.3Cindex.3E.3Cbegin.3E.3Clength.3E

## todo long-term
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
  * Maybe have a priority byte for each torrent. The client will make sure all of the higher pieces are downloading before continuing to download the lower pieces.

┌┐└┘├┤┬┴┼╭╮╰╯─│

Peer as duplex stream

peer.on<peer::Event::Handshake>(|event: HandshakeEvent| {
  ...
})

* what is a duplex stream?
* rust duplex streams

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
