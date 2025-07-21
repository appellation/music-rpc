use tauri::AppHandle;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tracing::Level;

use crate::{
	error::{AppError, AppResult},
	rpc::Rpc,
};

#[derive(Default)]
pub struct RpcState(OnceCell<Mutex<Rpc>>);

impl RpcState {
	#[tracing::instrument(skip(self, app), err, level = Level::INFO)]
	pub async fn get(
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

pub struct Config {
	pub client_id: u64,
	pub client_secret: String,
}
