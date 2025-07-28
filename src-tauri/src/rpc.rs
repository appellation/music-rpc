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
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, from_value, json, to_value};
use tauri::{AppHandle, async_runtime::spawn};
use tauri_plugin_store::StoreExt;
use tokio::{
	select,
	sync::{Mutex, mpsc, oneshot, watch},
};
use tokio_retry::{Retry, strategy::ExponentialBackoff};
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
	pub fn new(app: AppHandle, client_id: u64, client_secret: &'static str) -> AppResult<Self> {
		let connections = (0..10)
			.map(|id| Connection::new(app.clone(), id, client_id, client_secret))
			.collect();

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
	rq: reqwest::Client,
	pub tx: mpsc::Sender<CommandWithResponder>,
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
			status: Arc::new(Mutex::new(status_rx)),
			client_secret,
		};

		let retry_strategy = ExponentialBackoff::from_millis(10).max_delay(Duration::from_secs(60));

		// there's technically only one reference at any time, but the compiler complains
		let out_rx = Arc::new(Mutex::new(out_rx));
		let ready_tx2 = status_tx.clone();
		let rpc2 = rpc.clone();
		spawn(Retry::spawn(retry_strategy, move || {
			let rpc2 = rpc2.clone();
			let app = app.clone();
			let ready_tx2 = ready_tx2.clone();
			let out_rx = Arc::clone(&out_rx);
			let status_tx = status_tx.clone();

			async move {
				let result = rpc2.run(app, ready_tx2, out_rx).await;

				if let Err(err) = &result {
					if let Some(err) = err.0.downcast_ref::<io::Error>()
						&& err.kind() == ErrorKind::NotFound
					{
						// if we can't mark ourselves dead, nothing cares that we are dead
						let _ = status_tx.send(Status::Dead);
					} else {
						let _ = status_tx.send(Status::Opening);
					}
				}

				result
			}
		}));

		rpc
	}

	#[tracing::instrument(skip_all, fields(id = self.id), err(level = Level::DEBUG))]
	async fn run(
		self,
		app: AppHandle,
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

		let conn = self.clone();
		spawn(async move {
			conn.authenticate(app).await.unwrap();
			ready.send(Status::Open).unwrap();
		});

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

	#[tracing::instrument(skip_all, err)]
	pub async fn authorize(&self, client_secret: &str) -> AppResult<OAuth2Token> {
		let response = self
			.send(
				"AUTHORIZE",
				json!({
					"client_id": self.client_id.to_string(),
					"scopes": ["rpc", "rpc.activities.write"],
				}),
			)
			.await?;

		let res = from_value::<AuthorizeData>(response)?;
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

		self.send(
			"AUTHENTICATE",
			json!({ "access_token": token.access_token }),
		)
		.await?;

		Ok(token)
	}

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

		self.send(
			"AUTHENTICATE",
			json!({ "access_token": token.access_token }),
		)
		.await?;

		Ok(token)
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

#[must_use]
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
