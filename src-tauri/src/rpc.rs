use std::{fmt::Debug, process, time::Duration};

use anyhow::anyhow;
use codec::{Op, RpcCodec, RpcPacket};
use futures::{SinkExt, StreamExt, TryStream, TryStreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use tokio::{
	net::windows::named_pipe::ClientOptions,
	pin, spawn,
	sync::{mpsc, watch},
	time::sleep,
};
use tokio_stream::wrappers::WatchStream;
use tokio_util::codec::Framed;
use tracing::Level;
use ulid::Ulid;
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

use crate::error::{AppError, AppResult};

mod codec;

fn get_pipe_name(id: u8) -> String {
	format!(r#"\\?\pipe\discord-ipc-{}"#, id)
}

pub struct Rpc {
	tx: mpsc::Sender<RpcPacket>,
	rx: WatchStream<Option<RpcPacket>>,
	rq: reqwest::Client,
	client_id: u64,
}

impl Rpc {
	#[tracing::instrument(skip(client), err, level = Level::INFO)]
	pub async fn new(client: reqwest::Client, client_id: u64) -> AppResult<Self> {
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

		let mut framed = Framed::new(pipe, RpcCodec);
		framed
			.send(RpcPacket {
				op: Op::Handshake,
				data: json!({ "v": 1, "client_id": client_id.to_string() }),
			})
			.await?;

		let (tx, rx) = framed.split();
		let (in_tx, in_rx) = watch::channel(None);
		let (out_tx, out_rx) = mpsc::channel(32);

		spawn(Self::process_inbox(rx, out_tx.clone(), in_tx));
		spawn(Self::process_outbox(tx, out_rx));

		Ok(Self {
			tx: out_tx,
			rx: WatchStream::new(in_rx),
			rq: client,
			client_id,
		})
	}

	#[tracing::instrument(skip_all, err)]
	async fn process_inbox<S>(
		mut rx: S,
		sender: mpsc::Sender<RpcPacket>,
		receiver: watch::Sender<Option<RpcPacket>>,
	) -> AppResult<()>
	where
		S: TryStream<Ok = RpcPacket, Error = AppError> + Unpin,
	{
		while let Some(packet) = rx.try_next().await? {
			match packet.op {
				Op::Ping => {
					sender
						.send(RpcPacket {
							op: Op::Pong,
							data: packet.data,
						})
						.await?;
				}
				Op::Close => break,
				_ => {
					receiver.send_replace(Some(packet));
				}
			}
		}

		Ok::<_, AppError>(())
	}

	#[tracing::instrument(skip_all, err)]
	async fn process_outbox<S>(mut tx: S, mut receiver: mpsc::Receiver<RpcPacket>) -> AppResult<()>
	where
		S: SinkExt<RpcPacket, Error = AppError> + Unpin,
	{
		while let Some(packet) = receiver.recv().await {
			tx.send(packet).await?;
		}

		Ok(())
	}

	pub async fn authenticate(&mut self, client_secret: &str) -> AppResult<()> {
		let nonce = Ulid::new();
		self.send(&Command {
			nonce,
			args: AuthorizeArgs {
				client_id: self.client_id.to_string(),
				scopes: &["rpc", "rpc.activities.write"],
			},
			cmd: "AUTHORIZE",
		})
		.await?;

		let res = self.expect_response::<AuthorizeData>(Some(nonce)).await?;
		let res = self
			.rq
			.post("https://discord.com/api/v10/oauth2/token")
			.form(&OAuth2Body {
				grant_type: "authorization_code",
				code: res.data.code,
				client_id: self.client_id.to_string(),
				redirect_uri: "http://localhost",
			})
			.basic_auth(self.client_id, Some(client_secret))
			.send()
			.await?
			.error_for_status()?
			.json::<OAuth2Response>()
			.await?;

		self.send(&Command {
			nonce: Ulid::new(),
			args: AuthenticateArgs {
				access_token: res.access_token,
			},
			cmd: "AUTHENTICATE",
		})
		.await?;

		Ok(())
	}

	pub async fn set_activity(&mut self, activity: Activity) -> AppResult<()> {
		self.send(&Command {
			nonce: Ulid::new(),
			args: SetActivityArgs {
				pid: process::id(),
				activity,
			},
			cmd: "SET_ACTIVITY",
		})
		.await
	}

	#[tracing::instrument(skip(self), err, level = Level::DEBUG)]
	async fn send<T: Serialize + Debug>(&mut self, data: T) -> AppResult<()> {
		self.tx
			.send(RpcPacket {
				op: Op::Frame,
				data: serde_json::to_value(data)?,
			})
			.await?;

		Ok(())
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	pub async fn expect_response<T: DeserializeOwned + Debug>(
		&mut self,
		nonce: Option<Ulid>,
	) -> AppResult<Response<T>> {
		let stream = self.rx.by_ref().filter_map(|packet| async move {
			match packet {
				None => None,
				Some(packet) => {
					if packet.op != Op::Frame {
						return None;
					}

					let response = serde_json::from_value::<Response<T>>(packet.data).ok();
					if response.as_ref().and_then(|res| res.nonce) != nonce {
						None
					} else {
						response
					}
				}
			}
		});

		pin!(stream);
		stream
			.next()
			.await
			.ok_or_else(|| anyhow!("expected response, got none").into())
	}
}

#[derive(Debug, Serialize)]
struct Command<T> {
	pub nonce: Ulid,
	pub args: T,
	pub cmd: &'static str,
}

#[derive(Debug, Serialize)]
struct AuthorizeArgs {
	pub client_id: String,
	pub scopes: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct AuthenticateArgs {
	pub access_token: String,
}

#[derive(Debug, Serialize)]
pub struct SetActivityArgs {
	pub pid: u32,
	pub activity: Activity,
}

#[derive(Debug, Serialize, Default)]
pub struct Activity {
	pub name: String,
	pub r#type: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub url: Option<String>,
	pub created_at: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub timestamps: Option<ActivityTimestamps>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub application_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub details: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub state: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActivityTimestamps {
	pub start: Option<u128>,
	pub end: Option<u128>,
}

#[derive(Debug, Deserialize)]
pub struct Response<T> {
	pub nonce: Option<Ulid>,
	pub data: T,
	pub cmd: String,
}

#[derive(Debug, Deserialize)]
struct AuthorizeData {
	pub code: String,
}

#[derive(Debug, Serialize)]
struct OAuth2Body {
	pub grant_type: &'static str,
	pub code: String,
	pub client_id: String,
	pub redirect_uri: &'static str,
}

#[derive(Debug, Deserialize)]
struct OAuth2Response {
	pub access_token: String,
}
