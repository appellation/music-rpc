use std::{ffi::OsStr, path::PathBuf};

use anyhow::ensure;
use futures::{StreamExt, TryStream};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::{from_slice, from_str};
use tauri::{path::BaseDirectory, AppHandle, Manager};
use tokio::{
	io::{AsyncBufReadExt, BufReader},
	process::Command,
};
use tokio_stream::wrappers::LinesStream;

use crate::Properties;

pub struct MediaRemote {
	framework_path: PathBuf,
	script_path: PathBuf,
}

impl MediaRemote {
	pub fn new(app: AppHandle) -> Self {
		let framework_path = app
			.path()
			.resolve("MediaRemoteAdapter.framework", BaseDirectory::Resource)
			.expect("MediaRemoteAdapter.framework must be available on macOS");

		let script_path = app
			.path()
			.resolve("mediaremote-adapter.pl", BaseDirectory::Resource)
			.expect("mediaremote-adapter.pl must be available on macOS");

		Self {
			framework_path,
			script_path,
		}
	}

	pub async fn get_now_playing_info(&self) -> anyhow::Result<Option<NowPlayingInfo>> {
		let output = Command::new("/usr/bin/perl")
			.args([
				self.script_path.as_os_str(),
				self.framework_path.as_os_str(),
				OsStr::new("get"),
			])
			.output()
			.await?;
		ensure!(output.status.success(), "failed to get now playing info");

		Ok(from_slice(&output.stdout)?)
	}

	pub fn subscribe_now_playing_info(
		&self,
	) -> anyhow::Result<impl TryStream<Ok = NowPlayingInfo, Error = anyhow::Error>> {
		let cmd = Command::new("/usr/bin/perl")
			.args([
				self.script_path.as_os_str(),
				self.framework_path.as_os_str(),
				OsStr::new("stream"),
				OsStr::new("--no-diff"),
			])
			.spawn()?;

		let lines = LinesStream::new(BufReader::new(cmd.stdout.unwrap()).lines());

		Ok(lines.map(|line| {
			line.map_err(|err| anyhow::Error::from(err))
				.and_then(|inner| match from_str::<StreamPayload>(&inner) {
					Ok(payload) => Ok(payload.payload),
					Err(err) => Err(anyhow::Error::from(err)),
				})
		}))
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingInfo {
	pub bundle_identifier: String,
	pub playing: bool,
	pub title: String,
	pub artist: Option<String>,
	pub album: Option<String>,
	pub duration: Option<f32>,
	pub elapsed_time: Option<f32>,
	pub timestamp: Option<Timestamp>,
	pub artwork_mime_type: Option<String>,
	// pub artwork_data: Option<String>,
	pub chapter_number: Option<usize>,
}

impl From<NowPlayingInfo> for Properties {
	fn from(value: NowPlayingInfo) -> Self {
		let start = value.timestamp.unwrap().as_millisecond() as u128;
		let end = (value.timestamp.unwrap()
			+ SignedDuration::from_secs_f32(value.duration.unwrap()))
		.as_millisecond() as u128;

		Self {
			artist: value.artist.unwrap(),
			start,
			end,
			title: value.title,
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamPayload {
	pub diff: bool,
	pub payload: NowPlayingInfo,
}
