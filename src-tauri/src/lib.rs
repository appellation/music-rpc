use std::env;

use error::AppResult;
use tauri_plugin_autostart::MacosLauncher;
use tracing::Level;

use crate::{
	media::{serve::Server, Media},
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
	tracing_subscriber::fmt()
		.with_max_level(Level::DEBUG)
		.with_file(true)
		.with_line_number(true)
		.init();

	let config = Config {
		client_id: env!("CLIENT_ID")
			.parse()
			.expect("CLIENT_ID is not a number"),
		client_secret: env!("CLIENT_SECRET"),
	};

	tauri::Builder::default()
		.plugin(tauri_plugin_autostart::init(
			MacosLauncher::LaunchAgent,
			None,
		))
		.manage(Server::serve())
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
