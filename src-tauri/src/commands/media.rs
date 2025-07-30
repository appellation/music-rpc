use tauri::AppHandle;
use tracing::Level;

use crate::{
	error::AppResult,
	media::{self, Media},
};

#[tauri::command]
#[tracing::instrument(skip_all, ret, err, level = Level::INFO)]
pub async fn get_media(app: AppHandle) -> AppResult<Option<Media>> {
	media::get(app).await
}
