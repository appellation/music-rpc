use std::{fmt::Debug, process, time::Duration};

use codec::{Op, RpcCodec, RpcPacket};
use futures::{SinkExt, StreamExt};
use jiff::Timestamp;
use serde::{Serialize, Serializer};
use serde_json::{Value, json, to_value};
use tauri::async_runtime::spawn;
use tokio::{select, sync::mpsc, time::sleep};
use tokio_util::{codec::Framed, sync::CancellationToken};
use tracing::{Level, debug, warn};
use ulid::Ulid;

use crate::error::{AppError, AppResult};

mod codec;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

pub struct Rpc {
	connections: Vec<Connection>,
}

impl Rpc {
	#[tracing::instrument(err, level = Level::INFO)]
	pub fn new(client_id: u64) -> AppResult<Self> {
		let connections = (0..10).map(|id| Connection::new(id, client_id)).collect();

		Ok(Self { connections })
	}

	#[tracing::instrument(skip(self), err)]
	pub async fn set_activity(&self, activity: Activity) -> AppResult<()> {
		self.send_all(
			"SET_ACTIVITY",
			json!({ "pid": process::id(), "activity": activity }),
		)
		.await
	}

	#[tracing::instrument(skip(self), err)]
	pub async fn clear_activity(&self) -> AppResult<()> {
		self.send_all("SET_ACTIVITY", json!({ "pid": process::id() }))
			.await
	}

	#[tracing::instrument(skip_all, err, level = Level::DEBUG)]
	async fn send_all(&self, command: &'static str, args: Value) -> AppResult<()> {
		for conn in &self.connections {
			// we may fail to send for a variety of reasons that we want to ignore, including if the
			// connection is not yet open
			let _ = conn.send(command, args.clone());
		}

		Ok(())
	}
}

#[derive(Clone)]
struct Connection {
	pub tx: mpsc::Sender<Command>,
	done: CancellationToken,
}

impl Connection {
	#[tracing::instrument]
	pub fn new(id: u8, client_id: u64) -> Self {
		let (out_tx, mut out_rx) = mpsc::channel(4);
		let done = CancellationToken::new();

		let rpc = Self {
			tx: out_tx,
			done: done.clone(),
		};

		spawn(async move {
			loop {
				select! {
					_ = done.cancelled() => break,
					// we always want to retry, even (especially) if an error occurs.
					// we don't really care about the errors themselves.
					_ = Connection::run(id, client_id, &mut out_rx) => {
						sleep(Duration::from_secs(10)).await;
					}
				};
			}
		});

		rpc
	}
	#[tracing::instrument(skip(sender), err(level = Level::DEBUG))]
	async fn run(id: u8, client_id: u64, sender: &mut mpsc::Receiver<Command>) -> AppResult<()> {
		let pipe = Rpc::get_pipe(id).await?;
		let mut framed = Framed::new(pipe, RpcCodec::default());

		framed
			.send(RpcPacket {
				op: Op::Handshake,
				data: json!({ "v": 1, "client_id": client_id.to_string() }),
			})
			.await?;
		let _ready = framed.next().await.transpose()?;

		loop {
			let packet = select! {
				v = framed.next() => v.transpose()?,
				Some(cmd) = sender.recv() => {
					debug!(?cmd, "sending cmd");
					framed.send(RpcPacket { op: Op::Frame, data: to_value(cmd)? }).await?;
					continue;
				},
			};

			debug!(?packet, "received packet");

			let Some(packet) = packet else {
				break;
			};

			match packet.op {
				Op::Ping => {
					framed
						.send(RpcPacket {
							op: Op::Pong,
							data: packet.data,
						})
						.await?;
				}
				Op::Close => break,
				Op::Frame => {
					// we don't need to care about data from the rpc server
				}
				_ => {}
			}
		}

		Ok::<_, AppError>(())
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	fn send(&self, cmd: &'static str, args: Value) -> AppResult<()> {
		self.tx.try_send(Command {
			nonce: Ulid::new(),
			args,
			cmd,
		})?;
		Ok(())
	}
}

impl Drop for Connection {
	fn drop(&mut self) {
		self.done.cancel();
	}
}

#[derive(Debug, Clone, Serialize)]
struct Command {
	pub nonce: Ulid,
	pub args: Value,
	pub cmd: &'static str,
}

#[derive(Debug, Serialize, Default)]
pub struct Activity {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,
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
	#[serde(skip_serializing_if = "Option::is_none")]
	pub assets: Option<ActivityAssets>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub status_display_type: Option<u8>,
}

fn activity_timestamp_serializer<S>(
	timestamp: &Option<Timestamp>,
	serializer: S,
) -> Result<S::Ok, S::Error>
where
	S: Serializer,
{
	match timestamp {
		Some(timestamp) => serializer.serialize_i64(timestamp.as_millisecond()),
		None => serializer.serialize_none(),
	}
}

#[derive(Debug, Serialize)]
pub struct ActivityTimestamps {
	#[serde(serialize_with = "activity_timestamp_serializer")]
	pub start: Option<Timestamp>,
	#[serde(serialize_with = "activity_timestamp_serializer")]
	pub end: Option<Timestamp>,
}

#[derive(Debug, Serialize, Default)]
pub struct ActivityAssets {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub large_image: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub large_text: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub large_url: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub small_image: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub small_text: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub small_url: Option<String>,
}
