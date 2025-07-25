use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;

use crate::rpc::Rpc;

#[derive(Default)]
pub struct RpcState(OnceCell<Rpc>);

impl RpcState {
	pub async fn get(&self, app: AppHandle) -> &Rpc {
		let config = app.state::<Config>();
		let app_inner = app.clone();
		self.0
			.get_or_init(|| async move {
				Rpc::new(app_inner, config.client_id, config.client_secret).unwrap()
			})
			.await
	}
}

pub struct Config {
	pub client_id: u64,
	pub client_secret: &'static str,
}
