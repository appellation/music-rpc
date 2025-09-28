use anyhow::anyhow;
use tauri::State;

use crate::{
	api::Api,
	error::AppResult,
	media::Media,
	rpc::{Activity, ActivityAssets, ActivityTimestamps, Rpc},
	state::RpcState,
};

#[tauri::command]
pub async fn connect(rpc: State<'_, RpcState>, client_id: Option<String>) -> AppResult<bool> {
	match client_id {
		Some(client_id) => {
			let client_id = client_id.parse()?;
			let new_rpc = Rpc::new(client_id)?;
			*rpc.lock().await = Some(new_rpc);
			Ok(true)
		}
		None => {
			*rpc.lock().await = None;
			Ok(false)
		}
	}
}

#[tauri::command]
pub async fn set_activity(
	media: Option<Media>,
	rpc: State<'_, RpcState>,
	api: State<'_, Api>,
) -> AppResult<()> {
	let rpc = rpc.lock().await;
	let rpc = rpc
		.as_ref()
		.ok_or(anyhow!("must connect before setting activity"))?;

	match media {
		None => {
			rpc.clear_activity().await?;
		}
		Some(media) => {
			api.set_artwork(media.artwork_mime, media.artwork_bytes, media.end)
				.await?;

			rpc.set_activity(Activity {
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
