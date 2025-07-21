use std::env;

use error::{AppError, AppResult};
use futures::TryStreamExt;
use jiff::Timestamp;
use rpc::{Activity, ActivityTimestamps, Rpc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::{
	spawn,
	sync::{Mutex, MutexGuard, OnceCell},
};
use tracing::Level;

mod error;
mod media;
mod rpc;

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Properties {
	title: String,
	artist: String,
	start: Timestamp,
	end: Timestamp,
}

#[derive(Default)]
struct RpcState(OnceCell<Mutex<Rpc>>);

impl RpcState {
	#[tracing::instrument(skip(self, app), err, level = Level::INFO)]
	async fn get(
		&self,
		app: AppHandle,
		client_id: u64,
		client_secret: &str,
	) -> AppResult<MutexGuard<'_, Rpc>> {
		Ok(self
			.0
			.get_or_try_init(|| async {
				let mut rpc = Rpc::new(reqwest::Client::new(), client_id).await?;
				rpc.maybe_authenticate_all(app, client_secret).await?;
				Ok::<_, AppError>(Mutex::new(rpc))
			})
			.await?
			.lock()
			.await)
	}
}

#[tauri::command]
#[tracing::instrument(skip(app), ret, err, level = Level::INFO)]
async fn subscribe_media(app: AppHandle) -> AppResult<()> {
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
async fn get_media(app: AppHandle) -> AppResult<Option<Properties>> {
	media::get(app).await
}

#[tauri::command]
async fn set_activity(
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

struct Config {
	pub client_id: u64,
	pub client_secret: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> AppResult<()> {
	let _ = dotenvy::dotenv();

	tracing_subscriber::fmt()
		.with_max_level(Level::INFO)
		.with_file(true)
		.with_line_number(true)
		.init();

	let config = Config {
		client_id: env::var("CLIENT_ID")
			.expect("CLIENT_ID is not defined")
			.parse()
			.expect("CLIENT_ID is not a number"),
		client_secret: env::var("CLIENT_SECRET").expect("CLIENT_SECRET is not defined"),
	};

	tauri::Builder::default()
		.manage(RpcState::default())
		.manage(config)
		.plugin(tauri_plugin_store::Builder::new().build())
		.invoke_handler(tauri::generate_handler![
			get_media,
			subscribe_media,
			set_activity
		])
		.run(tauri::generate_context!())
		.expect("error while running tauri application");

	Ok(())
}
