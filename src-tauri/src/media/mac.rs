use std::sync::OnceLock;

use futures::{TryStream, TryStreamExt};
use media_remote::MediaRemote;

use tauri::AppHandle;

use crate::{error::AppResult, Media};

mod media_remote;

static MEDIA_REMOTE: OnceLock<MediaRemote> = OnceLock::new();

pub async fn get(app: AppHandle) -> AppResult<Option<Media>> {
	let mr = MEDIA_REMOTE.get_or_init(|| MediaRemote::new(app));
	let playing_info = mr.get_now_playing_info().await?;
	Ok(playing_info.and_then(|info| info.into()))
}

pub fn subscribe(
	app: AppHandle,
) -> anyhow::Result<impl TryStream<Ok = Option<Media>, Error = anyhow::Error>> {
	let mr = MEDIA_REMOTE.get_or_init(|| MediaRemote::new(app));
	Ok(mr.subscribe_now_playing_info()?.map_ok(|info| info.into()))
}
