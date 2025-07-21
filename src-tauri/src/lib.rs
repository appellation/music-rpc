use std::env;

use error::AppResult;
use tracing::Level;

use crate::{
	media::Properties,
	state::{Config, RpcState},
};

use commands::{
	media::{get_media, subscribe_media},
	rpc::set_activity,
};

mod commands;
mod error;
mod media;
mod rpc;
mod state;

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
