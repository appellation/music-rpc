use std::fmt::Debug;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

pub mod serve;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(windows)]
pub use win::*;

#[derive(Deserialize, Serialize, Clone)]
pub struct Properties {
	pub title: String,
	pub artist: String,
	pub start: Timestamp,
	pub end: Timestamp,
	#[serde(skip)]
	pub artwork_mime: String,
	#[serde(skip)]
	pub artwork_bytes: Vec<u8>,
	pub artwork_hash: String,
}

impl Debug for Properties {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Properties")
			.field("title", &self.title)
			.field("artist", &self.artist)
			.field("start", &self.start)
			.field("end", &self.end)
			.field("artwork_mime", &self.artwork_mime)
			.field("artwork_bytes", &"<bytes>")
			.field("artwork_hash", &self.artwork_hash)
			.finish()
	}
}
