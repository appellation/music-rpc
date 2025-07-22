use tauri::AppHandle;
use tokio::sync::{Mutex, MutexGuard, OnceCell};

use crate::{
	error::{AppError, AppResult},
	rpc::Rpc,
};

static RPC: OnceCell<Mutex<Rpc>> = OnceCell::const_new();

pub async fn rpc_client(
	app: AppHandle,
	client_id: u64,
	client_secret: &str,
) -> AppResult<MutexGuard<'_, Rpc>> {
	Ok(RPC
		.get_or_try_init(|| async {
			let mut rpc = Rpc::new(reqwest::Client::new(), client_id).await?;
			rpc.maybe_authenticate_all(app, client_secret).await?;
			Ok::<_, AppError>(Mutex::new(rpc))
		})
		.await?
		.lock()
		.await)
}

pub struct Config {
	pub client_id: u64,
	pub client_secret: &'static str,
}
