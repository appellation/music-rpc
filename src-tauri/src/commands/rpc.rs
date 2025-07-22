use tauri::{AppHandle, State};

use crate::{
	error::AppResult,
	media::{serve::Server, Properties},
	rpc::{Activity, ActivityAssets, ActivityTimestamps},
	state::{rpc_client, Config},
};

#[tauri::command]
#[tracing::instrument(skip(app, server, config), ret, err)]
pub async fn set_activity(
	app: AppHandle,
	properties: Option<Properties>,
	server: State<'_, Server>,
	config: State<'_, Config>,
) -> AppResult<()> {
	let rpc = rpc_client(app, config.client_id, &config.client_secret).await?;

	match properties {
		None => {
			rpc.clear_activity().await?;
		}
		Some(properties) => {
			rpc.set_activity(Activity {
				details: Some(properties.title),
				state: Some(properties.artist),
				r#type: 2,
				timestamps: Some(ActivityTimestamps {
					start: Some(properties.start),
					end: Some(properties.end),
				}),
				assets: server
					.public_url
					.lock()
					.await
					.clone()
					.map(|public_url| ActivityAssets {
						large_image: Some(format!("{}/{}", public_url, properties.artwork_hash)),
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
