use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(windows)]
pub use win::*;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Properties {
	pub title: String,
	pub artist: String,
	pub start: Timestamp,
	pub end: Timestamp,
}
