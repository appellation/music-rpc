fn get_pipe_name(id: u8) -> String {
	format!(r#"\\?\pipe\discord-ipc-{}"#, id)
}

impl Rpc {
	pub(crate) fn get_pipe() {
		let mut id = 0;
		let pipe = loop {
			match ClientOptions::new().open(get_pipe_name(id)) {
				Ok(client) => break client,
				Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
					sleep(Duration::from_millis(500)).await;
				}
				Err(e) => {
					if id == 10 {
						return Err(e.into());
					} else {
						id += 1;
					}
				}
			}
		};
	}
}
