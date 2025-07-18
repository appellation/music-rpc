use std::env;

use error::{AppError, AppResult};
use futures::TryStreamExt;
use rpc::{Activity, ActivityTimestamps, Rpc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
	start: u128,
	end: u128,
}

#[derive(Default)]
struct RpcState(OnceCell<Mutex<Rpc>>);

impl RpcState {
	#[tracing::instrument(skip(self), err, level = Level::INFO)]
	async fn get(&self, client_id: u64, client_secret: &str) -> AppResult<MutexGuard<'_, Rpc>> {
		Ok(self
			.0
			.get_or_try_init(|| async {
				let mut rpc = Rpc::new(reqwest::Client::new(), client_id).await?;
				rpc.expect_response::<Value>(None).await?;
				rpc.authenticate(client_secret).await?;
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
	properties: Properties,
	rpc: State<'_, RpcState>,
	config: State<'_, Config>,
) -> AppResult<()> {
	let mut rpc = rpc.get(config.client_id, &config.client_secret).await?;

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
		.with_max_level(Level::DEBUG)
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
		.invoke_handler(tauri::generate_handler![
			get_media,
			subscribe_media,
			set_activity
		])
		.run(tauri::generate_context!())
		.expect("error while running tauri application");

	Ok(())
}
