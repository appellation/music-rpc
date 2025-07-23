use std::{env, io};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::UnixStream;

use super::Rpc;

fn get_pipe(id: u8) -> String {
	let tmp = env::var("XDG_RUNTIME_DIR")
		.or(env::var("TMPDIR"))
		.or(env::var("TMP"))
		.or(env::var("TEMP"))
		.unwrap_or("/tmp".to_owned());

	format!("{}/discord-ipc-{}", tmp, id)
}

impl Rpc {
	pub(crate) async fn get_pipe(id: u8) -> io::Result<impl AsyncRead + AsyncWrite> {
		UnixStream::connect(get_pipe(id)).await
	}
}
