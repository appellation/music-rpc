use futures::TryStreamExt;
use tauri::{AppHandle, Emitter, async_runtime::spawn};
use tracing::Level;

use crate::{
	error::AppResult,
	media::{self, Media},
};

#[tauri::command]
#[tracing::instrument(skip_all, err, level = Level::INFO)]
pub async fn subscribe_media(app: AppHandle) -> AppResult<()> {
	let mut subscription = media::subscribe(app.clone()).await?;

	spawn(async move {
		while let Some(properties) = subscription.try_next().await.unwrap() {
			tracing::info!(?properties, "media change");
			app.emit("media_change", properties).unwrap();
		}
	});

	Ok(())
}

#[tauri::command]
#[tracing::instrument(skip_all, ret, err, level = Level::INFO)]
pub async fn get_media(app: AppHandle) -> AppResult<Option<Media>> {
	media::get(app).await
}
