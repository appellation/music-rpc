use std::{
	env, thread,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use error::{AppError, AppResult};
use rpc::{Activity, ActivityTimestamps, Rpc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{async_runtime::spawn, AppHandle, Emitter, State};
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tracing::Level;
use windows::{
	Foundation::TypedEventHandler,
	Media::Control::{
		GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
	},
};

mod error;
mod rpc;

fn universal_epoch() -> SystemTime {
	UNIX_EPOCH - Duration::from_secs(11_644_473_600)
}

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

#[tracing::instrument(skip_all, ret, level = Level::DEBUG)]
fn emit_media_change(app: AppHandle) {
	spawn(async move {
		app.emit("media_change", get_media().await.unwrap())
			.unwrap();
	});
}

#[tauri::command]
async fn subscribe_media(app: AppHandle) -> AppResult<()> {
	thread::spawn(move || {
		let session_manager =
			GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;

		let current_session = session_manager.GetCurrentSession()?;
		subscribe_session(app.clone(), current_session)?;

		// let session_changed_handler = TypedEventHandler::new(
		// 	move |sender: windows_core::Ref<GlobalSystemMediaTransportControlsSessionManager>, _| {
		// 		if let Some(manager) = &*sender {
		// 			let session = manager.GetCurrentSession().unwrap();
		// 			// subscribe_session(app.clone(), session).unwrap();
		// 		}

		// 		println!("session changed");
		// 		emit_media_change(app.clone());
		// 		Ok(())
		// 	},
		// );
		// session_manager.CurrentSessionChanged(&session_changed_handler)?;

		Ok::<_, AppError>(())
	});

	Ok(())
}

fn subscribe_session(
	app: AppHandle,
	session: GlobalSystemMediaTransportControlsSession,
) -> AppResult<()> {
	let app2 = app.clone();
	let timeline_properties_changed_handler = TypedEventHandler::new(move |_, _| {
		emit_media_change(app2.clone());
		Ok(())
	});
	let _timeline_token =
		session.TimelinePropertiesChanged(&timeline_properties_changed_handler)?;

	let media_properties_changed_handler = TypedEventHandler::new(move |_, _| {
		emit_media_change(app.clone());
		Ok(())
	});
	let _media_properties_token =
		session.MediaPropertiesChanged(&media_properties_changed_handler)?;

	thread::park();
	Ok(())
}

#[tauri::command]
#[tracing::instrument(ret, err, level = Level::INFO)]
async fn get_media() -> AppResult<Properties> {
	let session = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?
		.await?
		.GetCurrentSession()?;

	let properties = session.TryGetMediaPropertiesAsync()?.await?;
	let timeline = session.GetTimelineProperties()?;

	let last_updated = universal_epoch()
		+ Duration::from_nanos(timeline.LastUpdatedTime()?.UniversalTime as u64 * 100);
	let start =
		last_updated + Duration::from(timeline.StartTime()?) - Duration::from(timeline.Position()?);
	let end = start + Duration::from(timeline.EndTime()?);

	Ok(Properties {
		title: properties.Title()?.to_string_lossy(),
		artist: properties.Artist()?.to_string_lossy(),
		start: start.duration_since(UNIX_EPOCH)?.as_millis(),
		end: end.duration_since(UNIX_EPOCH)?.as_millis(),
	})
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
			.expect("CLIENT_ID is defined")
			.parse()
			.expect("CLIENT_ID is a number"),
		client_secret: env::var("CLIENT_SECRET").expect("CLIENT_SECRET is defined"),
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
