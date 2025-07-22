use futures::TryStreamExt;
use tauri::{async_runtime::spawn, AppHandle, Emitter, State};
use tracing::Level;

use crate::{
	error::AppResult,
	media::{self, serve::Server, Properties},
};

#[tauri::command]
#[tracing::instrument(skip_all, err, level = Level::INFO)]
pub async fn subscribe_media(app: AppHandle, server: State<'_, Server>) -> AppResult<()> {
	let mut subscription = media::subscribe(app.clone())?;

	let server2 = server.inner().clone();
	spawn(async move {
		while let Some(properties) = subscription.try_next().await.unwrap() {
			tracing::info!(?properties, "media change");
			if let Some(properties) = &properties {
				server2
					.set_artwork(
						properties.artwork_mime.clone(),
						properties.artwork_bytes.clone(),
					)
					.await;
			}
			app.emit("media_change", properties).unwrap();
		}
	});

	Ok(())
}

#[tauri::command]
#[tracing::instrument(skip_all, ret, err, level = Level::INFO)]
pub async fn get_media(app: AppHandle, server: State<'_, Server>) -> AppResult<Option<Properties>> {
	let properties = media::get(app).await?;
	if let Some(properties) = properties.as_ref() {
		server
			.set_artwork(
				properties.artwork_mime.clone(),
				properties.artwork_bytes.clone(),
			)
			.await;
	}

	Ok(properties)
}
