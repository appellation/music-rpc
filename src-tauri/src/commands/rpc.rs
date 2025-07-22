use tauri::{AppHandle, State};

use crate::{
	error::AppResult,
	media::{serve::Server, Media},
	rpc::{Activity, ActivityAssets, ActivityTimestamps},
	state::RpcState,
};

#[tauri::command]
pub async fn set_activity(
	app: AppHandle,
	media: Option<Media>,
	rpc: State<'_, RpcState>,
	server: State<'_, Server>,
) -> AppResult<()> {
	match media {
		None => {
			rpc.get(app).await.clear_activity().await?;
		}
		Some(media) => {
			server
				.set_artwork(media.artwork_mime, media.artwork_bytes)
				.await;

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
						large_image: Some(format!(
							"{}/{}",
							server.public_url().await,
							media.artwork_hash
						)),
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
