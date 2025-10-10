use std::{
	collections::HashMap,
	fmt::Debug,
	io::{self, ErrorKind},
	process,
	sync::Arc,
	time::Duration,
};

use codec::{Op, RpcCodec, RpcPacket};
use futures::{SinkExt, Stream, StreamExt, stream};
use jiff::Timestamp;
use serde::{Serialize, Serializer};
use serde_json::{Value, from_value, json, to_value};
use tauri::async_runtime::spawn;
use tokio::{
	select,
	sync::{Mutex, Notify, mpsc, oneshot, watch},
	time::sleep,
};
use tokio_util::codec::Framed;
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
		let mut connections = self.open_connections();
		while let Some(conn) = connections.next().await {
			conn.send(command, args.clone()).await?;
		}

		Ok(())
	}

	fn open_connections(&self) -> impl Stream<Item = &Connection> + Unpin {
		let stream = stream::iter(self.connections.iter()).filter(|conn| async {
			let mut status_rx = conn.status.lock().await;

			loop {
				let status = *status_rx.borrow();
				match status {
					Status::Dead => return false,
					Status::Open => return true,
					Status::Opening => {
						status_rx.changed().await.unwrap();
					}
				}
			}
		});
		Box::pin(stream)
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Status {
	Opening,
	Open,
	Dead,
}

type CommandWithResponder = (oneshot::Sender<RpcPacket>, Command);

#[derive(Clone)]
struct Connection {
	pub id: u8,
	pub client_id: u64,
	pub tx: mpsc::Sender<CommandWithResponder>,
	pub status: Arc<Mutex<watch::Receiver<Status>>>,
	pub done: Arc<Notify>,
}

impl Connection {
	#[tracing::instrument]
	pub fn new(id: u8, client_id: u64) -> Self {
		let (out_tx, out_rx) = mpsc::channel(32);
		let (status_tx, status_rx) = watch::channel(Status::Opening);
		let done = Arc::new(Notify::new());

		let rpc = Self {
			id,
			client_id,
			tx: out_tx,
			status: Arc::new(Mutex::new(status_rx)),
			done: Arc::clone(&done),
		};

		// there's technically only one reference at any time, but the compiler complains
		let out_rx = Arc::new(Mutex::new(out_rx));
		let ready_tx2 = status_tx.clone();
		let rpc2 = rpc.clone();

		spawn(async move {
			loop {
				let result = select! {
					_ = done.notified() => break,
					result = rpc2.clone().run(ready_tx2.clone(), out_rx.clone()) => result
				};

				match result {
					Err(err) => {
						if let Some(err) = err.0.downcast_ref::<io::Error>()
							&& err.kind() == ErrorKind::NotFound
						{
							// if we can't mark ourselves dead, nothing cares that we are dead
							let _ = status_tx.send(Status::Dead);
						} else {
							let _ = status_tx.send(Status::Opening);
						}
					}
					Ok(()) => {
						let _ = status_tx.send(Status::Opening);
					}
				}

				sleep(Duration::from_secs(10)).await;
			}
		});

		rpc
	}

	#[tracing::instrument(skip_all, fields(id = self.id), err(level = Level::DEBUG))]
	async fn run(
		self,
		ready: watch::Sender<Status>,
		sender: Arc<Mutex<mpsc::Receiver<CommandWithResponder>>>,
	) -> AppResult<()> {
		let pipe = Rpc::get_pipe(self.id).await?;
		let mut framed = Framed::new(pipe, RpcCodec::default());

		framed
			.send(RpcPacket {
				op: Op::Handshake,
				data: json!({ "v": 1, "client_id": self.client_id.to_string() }),
			})
			.await?;
		let _ready = framed.next().await.transpose()?;
		ready.send(Status::Open).unwrap();

		let mut expected = HashMap::new();

		loop {
			let mut sender = sender.lock().await;
			let packet = select! {
				v = framed.next() => v.transpose()?,
				Some((done, cmd)) = sender.recv() => {
					debug!(?cmd, "sending cmd");

					expected.insert(cmd.nonce, done);
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
					let nonce = from_value::<Ulid>(packet.data.get("nonce").unwrap().clone())?;
					if let Some(sender) = expected.remove(&nonce) {
						// we can assume that a failure to send means that nothing cares about this response
						let _ = sender.send(packet);
					}
				}
				_ => {}
			}
		}

		Ok::<_, AppError>(())
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	async fn send(&self, cmd: &'static str, args: Value) -> AppResult<Value> {
		let (tx, rx) = oneshot::channel();
		self.tx
			.send((
				tx,
				Command {
					nonce: Ulid::new(),
					args,
					cmd,
				},
			))
			.await?;
		Ok(rx.await?.data.get_mut("data").unwrap().take())
	}
}

impl Drop for Connection {
	fn drop(&mut self) {
		self.done.notify_one();
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
