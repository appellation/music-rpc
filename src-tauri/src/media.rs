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
pub struct Media {
	pub title: String,
	pub artist: String,
	pub start: Timestamp,
	pub end: Timestamp,
	pub artwork_mime: String,
	#[serde(with = "artwork_bytes")]
	pub artwork_bytes: Vec<u8>,
	pub artwork_hash: String,
}

impl Debug for Media {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Media")
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

mod artwork_bytes {
	use base64::{prelude::BASE64_STANDARD, Engine};
	use serde::{de::Visitor, Deserializer, Serializer};

	pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let encoded = BASE64_STANDARD.encode(bytes);
		serializer.serialize_str(&encoded)
	}

	pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
	where
		D: Deserializer<'de>,
	{
		struct Base64Visitor;

		impl<'de> Visitor<'de> for Base64Visitor {
			type Value = Vec<u8>;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("base64 string")
			}

			fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
			where
				E: serde::de::Error,
			{
				BASE64_STANDARD.decode(v).map_err(|e| E::custom(e))
			}
		}

		deserializer.deserialize_str(Base64Visitor)
	}
}
