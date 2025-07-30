use std::env;

use error::AppResult;
use tauri::{
	Manager,
	menu::{Menu, MenuItem},
	tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::MacosLauncher;
use tracing::Level;

use crate::{
	api::Api,
	state::{Config, RpcState},
};

use commands::{
	media::{get_media, subscribe_media},
	rpc::set_activity,
};

mod api;
mod commands;
mod error;
mod media;
mod rpc;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> AppResult<()> {
	#[cfg(debug_assertions)]
	tracing_subscriber::fmt()
		.with_max_level(Level::DEBUG)
		.with_file(true)
		.with_line_number(true)
		.init();

	let config = Config {
		client_id: env!("CLIENT_ID")
			.parse()
			.expect("CLIENT_ID is not a number"),
	};

	tauri::Builder::default()
		.setup(|app| {
			let quit = MenuItem::new(app, "Quit", true, None::<&str>)?;
			let show = MenuItem::new(app, "Show", true, None::<&str>)?;
			let menu = Menu::with_items(app, &[&show, &quit])?;

			let _tray = TrayIconBuilder::new()
				.icon(app.default_window_icon().unwrap().clone())
				.menu(&menu)
				.on_menu_event(move |app, event| match event.id {
					id if id == quit.id() => {
						app.exit(0);
					}
					id if id == show.id() => {
						if let Some(window) = app.get_webview_window("main") {
							let _ = window.show();
							let _ = window.set_focus();
						}
					}
					_ => {}
				})
				.on_tray_icon_event(|icon, event| {
					if let TrayIconEvent::Click {
						button: MouseButton::Left,
						..
					} = event
					{
						if let Some(window) = icon.app_handle().get_webview_window("main") {
							let _ = window.show();
							let _ = window.set_focus();
						}
					}
				})
				.build(app)?;

			Ok(())
		})
		.on_window_event(|window, event| {
			if let tauri::WindowEvent::CloseRequested { api, .. } = event {
				window.hide().unwrap();
				api.prevent_close();
			}
		})
		.plugin(tauri_plugin_autostart::init(
			MacosLauncher::LaunchAgent,
			None,
		))
		.manage(Api::new(env!("API_URL")))
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
