use std::env;

use error::AppResult;
use tauri::Manager;
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
		.setup(|app| {
			app.manage(Server::serve(app.handle().to_owned()).unwrap());
			app.manage(RpcState::default());
			Ok(())
		})
		.manage(config)
		.plugin(tauri_plugin_store::Builder::new().build())
		.plugin(tauri_plugin_shell::init())
		.invoke_handler(tauri::generate_handler![
			get_media,
			subscribe_media,
			set_activity
		])
		.run(tauri::generate_context!())
		.expect("error while running tauri application");

	Ok(())
}
