use std::time::Duration;

use anyhow::anyhow;
use tokio::{
	io::{AsyncRead, AsyncWrite},
	net::windows::named_pipe::{ClientOptions, NamedPipeClient},
	time::sleep,
};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

use crate::{error::AppResult, rpc::Rpc};

fn get_pipe_name(id: u8) -> String {
	format!(r#"\\?\pipe\discord-ipc-{}"#, id)
}

impl Rpc {
	pub(crate) async fn get_pipe(id: u8) -> AppResult<impl AsyncRead + AsyncWrite> {
		loop {
			match ClientOptions::new().open(get_pipe_name(id)) {
				Ok(client) => break Ok(client),
				Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
					sleep(Duration::from_millis(500)).await;
				}
				Err(e) => break Err::<NamedPipeClient, _>(anyhow!(e).into()),
			}
		}
	}
}
