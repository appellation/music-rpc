use tauri::{AppHandle, State};

use crate::{
	api::Api,
	error::AppResult,
	media::Media,
	rpc::{Activity, ActivityAssets, ActivityTimestamps},
	state::RpcState,
};

#[tauri::command]
pub async fn set_activity(
	app: AppHandle,
	media: Option<Media>,
	rpc: State<'_, RpcState>,
	api: State<'_, Api>,
) -> AppResult<()> {
	match media {
		None => {
			rpc.get(app).await.clear_activity().await?;
		}
		Some(media) => {
			api.set_artwork(media.artwork_mime, media.artwork_bytes, media.end)
				.await?;

			rpc.get(app)
				.await
				.set_activity(Activity {
					details: Some(media.title),
					state: Some(media.artist),
					r#type: 2,
					timestamps: Some(ActivityTimestamps {
						start: Some(media.start),
						end: Some(media.end),
					}),
					assets: Some(ActivityAssets {
						large_image: Some(format!("{}/{}", api.base_url, media.artwork_hash)),
						..Default::default()
					}),
					status_display_type: Some(1),
					..Default::default()
				})
				.await?;
		}
	}

	Ok(())
}
