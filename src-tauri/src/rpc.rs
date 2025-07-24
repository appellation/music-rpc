use std::{
	collections::HashMap,
	fmt::Debug,
	io::{self, ErrorKind},
	process,
	sync::Arc,
	time::Duration,
};

use anyhow::anyhow;
use codec::{Op, RpcCodec, RpcPacket};
use futures::{stream, SinkExt, Stream, StreamExt};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{from_value, json, to_value, Value};
use tauri::{async_runtime::spawn, AppHandle};
use tauri_plugin_store::StoreExt;
use tokio::{
	select,
	sync::{mpsc, oneshot, watch, Mutex},
};
use tokio_retry::{strategy::ExponentialBackoff, RetryIf};
use tokio_util::codec::Framed;
use tracing::Level;
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
	pub fn new(app: AppHandle, client_id: u64, client_secret: &'static str) -> AppResult<Self> {
		let connections = (0..10)
			.map(|id| Connection::new(app.clone(), id, client_id, client_secret))
			.collect();

		Ok(Self { connections })
	}

	#[tracing::instrument(skip(self), err)]
	pub async fn set_activity(&self, activity: Activity) -> AppResult<()> {
		self.send_all(Command {
			nonce: Ulid::new(),
			args: json!({ "pid": process::id(), "activity": activity }),
			cmd: "SET_ACTIVITY",
		})
		.await
	}

	#[tracing::instrument(skip(self), err)]
	pub async fn clear_activity(&self) -> AppResult<()> {
		self.send_all(Command {
			nonce: Ulid::new(),
			args: json!({ "pid": process::id() }),
			cmd: "SET_ACTIVITY",
		})
		.await
	}

	#[tracing::instrument(skip_all, err, level = Level::DEBUG)]
	async fn send_all(&self, data: Command) -> AppResult<()> {
		let mut connections = self.open_connections();
		while let Some(conn) = connections.next().await {
			conn.send(data.clone()).await?;
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

#[derive(Clone)]
struct Connection {
	pub id: u8,
	pub client_id: u64,
	rq: reqwest::Client,
	pub tx: mpsc::Sender<Command>,
	pub rx: Arc<Mutex<HashMap<Ulid, oneshot::Receiver<RpcPacket>>>>,
	pub status: Arc<Mutex<watch::Receiver<Status>>>,
	client_secret: &'static str,
}

impl Connection {
	#[tracing::instrument]
	pub fn new(app: AppHandle, id: u8, client_id: u64, client_secret: &'static str) -> Self {
		let (out_tx, out_rx) = mpsc::channel(32);
		let (status_tx, status_rx) = watch::channel(Status::Opening);

		let rpc = Self {
			id,
			client_id,
			rq: reqwest::Client::new(),
			tx: out_tx,
			rx: Arc::default(),
			status: Arc::new(Mutex::new(status_rx)),
			client_secret,
		};

		let retry_strategy = ExponentialBackoff::from_millis(10).max_delay(Duration::from_secs(60));

		// there's technically only one reference at any time, but the compiler complains
		let out_rx = Arc::new(Mutex::new(out_rx));
		let ready_tx2 = status_tx.clone();
		let rpc2 = rpc.clone();
		spawn(RetryIf::spawn(
			retry_strategy,
			move || {
				let out_rx = Arc::clone(&out_rx);
				rpc2.clone().run(app.clone(), ready_tx2.clone(), out_rx)
			},
			move |err: &AppError| {
				if let Some(err) = err.0.downcast_ref::<io::Error>() {
					if err.kind() == ErrorKind::NotFound {
						// if we can't mark ourselves dead, nothing cares that we are dead
						let _ = status_tx.send(Status::Dead);
						return false;
					}
				}

				true
			},
		));

		rpc
	}

	#[tracing::instrument(skip_all, fields(id = self.id), err(level = Level::WARN))]
	async fn run(
		self,
		app: AppHandle,
		ready: watch::Sender<Status>,
		sender: Arc<Mutex<mpsc::Receiver<Command>>>,
	) -> AppResult<()> {
		ready.send(Status::Opening)?;

		let pipe = Rpc::get_pipe(self.id).await?;
		let mut framed = Framed::new(pipe, RpcCodec::default());

		framed
			.send(RpcPacket {
				op: Op::Handshake,
				data: json!({ "v": 1, "client_id": self.client_id.to_string() }),
			})
			.await?;
		framed.next().await;

		self.authenticate(app).await?;
		ready.send(Status::Open)?;

		let mut expected = HashMap::new();

		loop {
			let mut sender = sender.lock().await;
			let packet = select! {
				v = framed.next() => v.transpose()?,
				Some(cmd) = sender.recv() => {
					let (res_tx, res_rx) = oneshot::channel();
					expected.insert(cmd.nonce, res_tx);
					self.rx.lock().await.insert(cmd.nonce, res_rx);
					framed.send(RpcPacket { op: Op::Frame, data: to_value(cmd)? }).await?;
					continue;
				},
			};

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

	pub async fn authenticate(&self, app: AppHandle) -> AppResult<()> {
		let token = app.store("auth")?.get(self.id.to_string());
		let mut token = match token {
			None => self.authorize(self.client_secret).await?,
			Some(value) => serde_json::from_value(value)?,
		};

		if token.expires_at < Timestamp::now() {
			token = self.authorize(self.client_secret).await?;
		} else if token.expires_at < Timestamp::now() + SignedDuration::from_hours(24) {
			token = self.refresh(token, self.client_secret).await?;
		}

		app.store("auth")?
			.set(self.id.to_string(), serde_json::to_value(token)?);
		Ok(())
	}

	#[must_use]
	#[tracing::instrument(skip_all, err)]
	pub async fn authorize(&self, client_secret: &str) -> AppResult<OAuth2Token> {
		let nonce = Ulid::new();
		self.send(Command {
			nonce,
			args: json!({
				"client_id": self.client_id.to_string(),
				"scopes": ["rpc", "rpc.activities.write"],
			}),
			cmd: "AUTHORIZE",
		})
		.await?;

		let res = from_value::<AuthorizeData>(self.expect_response(nonce).await?)?;
		let res = self
			.rq
			.post("https://discord.com/api/v10/oauth2/token")
			.form(&OAuth2Body {
				grant_type: "authorization_code",
				code: res.code,
				client_id: self.client_id.to_string(),
				redirect_uri: "http://localhost",
			})
			.basic_auth(self.client_id, Some(client_secret))
			.send()
			.await?
			.error_for_status()?
			.json::<OAuth2Response>()
			.await?;
		let token: OAuth2Token = res.into();

		self.send(Command {
			nonce: Ulid::new(),
			args: json!({ "access_token": token.access_token }),
			cmd: "AUTHENTICATE",
		})
		.await?;

		Ok(token)
	}

	#[must_use]
	#[tracing::instrument(skip_all, err)]
	async fn refresh(&self, token: OAuth2Token, client_secret: &str) -> AppResult<OAuth2Token> {
		let res = self
			.rq
			.post("https://discord.com/api/v10/oauth2/token")
			.form(&OAuth2RefreshBody {
				grant_type: "refresh_token",
				refresh_token: token.refresh_token,
			})
			.basic_auth(self.client_id, Some(client_secret))
			.send()
			.await?
			.error_for_status()?
			.json::<OAuth2Response>()
			.await?;
		let token: OAuth2Token = res.into();

		self.send(Command {
			nonce: Ulid::new(),
			args: json!({ "access_token": token.access_token }),
			cmd: "AUTHENTICATE",
		})
		.await?;

		Ok(token)
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	async fn send(&self, data: Command) -> AppResult<()> {
		// TODO: somehow this channel can close
		self.tx.send(data).await?;
		Ok(())
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	pub async fn expect_response(&self, expected: Ulid) -> AppResult<Value> {
		let value = self
			.rx
			.lock()
			.await
			.remove(&expected)
			.ok_or(anyhow!("no listener for nonce {}", expected))?
			.await?;

		Ok(value.data)
	}
}

#[derive(Debug, Clone, Serialize)]
struct Command {
	pub nonce: Ulid,
	pub args: Value,
	pub cmd: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SetActivityArgs {
	pub pid: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub activity: Option<Activity>,
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

#[derive(Debug, Serialize)]
struct OAuth2RefreshBody {
	pub grant_type: &'static str,
	pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuth2Response {
	pub access_token: String,
	pub expires_in: u64,
	pub refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuth2Token {
	pub access_token: String,
	pub expires_at: Timestamp,
	pub refresh_token: String,
}

impl From<OAuth2Response> for OAuth2Token {
	fn from(value: OAuth2Response) -> Self {
		Self {
			access_token: value.access_token,
			expires_at: Timestamp::now() + Duration::from_secs(value.expires_in),
			refresh_token: value.refresh_token,
		}
	}
}
