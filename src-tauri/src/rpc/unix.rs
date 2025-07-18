use std::env;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::UnixStream;

use crate::error::AppResult;

use super::{try_all_pipes, Rpc};

fn get_pipe(id: u8) -> String {
	let tmp = env::var("XDG_RUNTIME_DIR")
		.or(env::var("TMPDIR"))
		.or(env::var("TMP"))
		.or(env::var("TEMP"))
		.unwrap_or("/tmp".to_owned());

	format!("{}/discord-ipc-{}", tmp, id)
}

impl Rpc {
	pub(crate) async fn get_pipe() -> AppResult<impl AsyncRead + AsyncWrite> {
		Ok(try_all_pipes(|id| UnixStream::connect(get_pipe(id)), |_| false).await?)
	}
}
