# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

## todo
* Use an event-driven arcitecture (still with a work queue)
* See https://blog.jse.li/posts/torrent/
* See https://en.wikipedia.org/wiki/Torrent_file
* See https://www.bittorrent.org/beps/bep_0003.html
* See http://www.kristenwidman.com/blog/71/how-to-write-a-bittorrent-client-part-2/
* See https://wiki.theory.org/BitTorrentSpecification#cancel:_.3Clen.3D0013.3E.3Cid.3D8.3E.3Cindex.3E.3Cbegin.3E.3Clength.3E

## todo long-term
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
  * Maybe have a priority byte for each torrent. The client will make sure all of the higher pieces are downloading before continuing to download the lower pieces.

┌┐└┘├┤┬┴┼╭╮╰╯─│

TorrentAdded ─── Connect to peers and add work
┌─ WorkAdded ─── All peer threads check for work
├─ AvailablePiecesUpdated ─── That peer checks for work
╰─── If work is found, pop it and start downloading

fuck this do later


TODO: We need to run async code in callbacks
