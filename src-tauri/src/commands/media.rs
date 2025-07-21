use futures::TryStreamExt;
use tauri::{async_runtime::spawn, AppHandle, Emitter};
use tracing::Level;

use crate::{
	error::AppResult,
	media::{self, Properties},
};

#[tauri::command]
#[tracing::instrument(skip(app), ret, err, level = Level::INFO)]
pub async fn subscribe_media(app: AppHandle) -> AppResult<()> {
	let mut subscription = media::subscribe(app.clone())?;

	spawn(async move {
		while let Some(properties) = subscription.try_next().await.unwrap() {
			tracing::info!(?properties, "media change");
			app.emit("media_change", properties).unwrap();
		}
	});

	Ok(())
}

#[tauri::command]
#[tracing::instrument(skip(app), ret, err, level = Level::INFO)]
pub async fn get_media(app: AppHandle) -> AppResult<Option<Properties>> {
	media::get(app).await
}
