use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;

use crate::rpc::Rpc;

#[derive(Default)]
pub struct RpcState(OnceCell<Rpc>);

impl RpcState {
	pub async fn get(&self, app: AppHandle) -> &Rpc {
		let config = app.state::<Config>();
		self.0
			.get_or_init(|| async move { Rpc::new(config.client_id).unwrap() })
			.await
	}
}

pub struct Config {
	pub client_id: u64,
}
