//! A bittorrent client prioritizing simplicity, reliability and correctness
//!
//! # Getting started
//! 1. Create a [`Session`](core::session::Session)
//! 2. Load the [`MetaInfo`](protocol::metainfo::MetaInfo) from a torrent file
//! 3. Create a [`Store`](io::store::Store) to store the downloaded data
//! 4. Add the meta info and store to the session with [`Session::add`](core::session::Session::add)
//! 5. Start the session with [`Session::start`](core::session::Session::start)
//!
//! # Example
//! ```rust,no_run
//! use bittorrent::{
//! 	core::session::Session,
//! 	protocol::metainfo::MetaInfo,
//! 	io::store::FileStore
//! };
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() {
//!     let mut session = Session::new([b'x'; 20]);
//!     let meta_info = MetaInfo::load("sample.torrent").unwrap();
//!
//!     let store = FileStore::new(
//!         meta_info.piece_size,
//!         meta_info.files
//!             .iter()
//!             .map(|file| (file.length, file.path.clone()))
//!             .collect::<Vec<_>>()
//!     )
//!     .unwrap();
//!
//!     session.add(meta_info, Box::new(store));
//!     session.start().await;
//! }
//! ```

#![feature(trait_alias)]
#![feature(duration_consts_float)]
#![feature(int_roundings)]

pub mod core;
pub mod io;
pub mod protocol;
