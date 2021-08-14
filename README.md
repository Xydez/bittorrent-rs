# bittorrent-rs
bittorrent-rs is a lightweight implementation of the bittorrent v1 protocol as described in the [BitTorrent Protocol Specification](https://www.bittorrent.org/beps/bep_0003.html)

## todo
* Bittorrent Enhancement Proposals
  * BEP-0012 - Multiple trackers
  * BEP-0017 - Http seeds
  * See https://en.wikipedia.org/wiki/Torrent_file
  * See https://www.bittorrent.org/beps/bep_0000.html

## long-term todo
* Allow requesting specific byte ranges from the torrent, and the client will prioritize those pieces
  * Maybe have a signed priority byte for each torrent that ranges from -128 to 127. The client will make sure all of the higher pieces are downloading before continuing to download the lower pieces.
