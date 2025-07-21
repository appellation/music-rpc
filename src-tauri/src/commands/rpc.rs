use tauri::{AppHandle, State};

use crate::{
	error::AppResult,
	media::Properties,
	rpc::{Activity, ActivityTimestamps},
	state::{Config, RpcState},
};

#[tauri::command]
pub async fn set_activity(
	app: AppHandle,
	properties: Option<Properties>,
	rpc: State<'_, RpcState>,
	config: State<'_, Config>,
) -> AppResult<()> {
	let rpc = rpc
		.get(app, config.client_id, &config.client_secret)
		.await?;

	match properties {
		None => {
			rpc.clear_activity().await?;
		}
		Some(properties) => {
			rpc.set_activity(Activity {
				details: Some(properties.artist),
				state: Some(properties.title),
				r#type: 2,
				timestamps: Some(ActivityTimestamps {
					start: Some(properties.start),
					end: Some(properties.end),
				}),
				..Default::default()
			})
			.await?;
		}
	}

	Ok(())
}
