use std::{collections::HashMap, fmt::Debug, future::Future, process, time::Duration};

use anyhow::anyhow;
use codec::{Op, RpcCodec, RpcPacket};
use futures::{SinkExt, StreamExt, TryStream, TryStreamExt};
use jiff::{SignedDuration, Timestamp};
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::{
	spawn,
	sync::{mpsc, watch, Mutex},
	time::sleep,
};
use tokio_util::codec::Framed;
use tracing::Level;
use ulid::Ulid;

use crate::error::{AppError, AppResult};

mod codec;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

async fn try_all_pipes<F, C, T, E>(
	get_client: F,
	try_again: impl Fn(&E) -> bool,
) -> AppResult<HashMap<u8, T>>
where
	F: Fn(u8) -> C,
	C: Future<Output = Result<T, E>>,
	E: Into<AppError> + std::error::Error + Send + Sync + 'static,
{
	let mut clients = HashMap::new();
	for id in 0..10 {
		match get_client(id).await {
			Ok(client) => {
				clients.insert(id, client);
			}
			Err(e) if try_again(&e) => {
				sleep(Duration::from_millis(500)).await;
			}
			Err(e) => {
				tracing::debug!(
					"failed to connect to client ID {}: {}",
					id,
					AppError::from(e)
				);
			}
		}
	}

	Ok(clients)
}

pub struct Rpc {
	tx: HashMap<u8, mpsc::Sender<RpcPacket>>,
	rx: HashMap<u8, Mutex<watch::Receiver<Option<RpcPacket>>>>,
	rq: reqwest::Client,
	client_id: u64,
}

impl Rpc {
	#[tracing::instrument(skip(client), err, level = Level::INFO)]
	pub async fn new(client: reqwest::Client, client_id: u64) -> AppResult<Self> {
		let pipes = Self::get_pipes().await?;

		let mut senders = HashMap::new();
		let mut receivers = HashMap::new();

		for (id, pipe) in pipes {
			let mut framed = Framed::new(pipe, RpcCodec);
			framed
				.send(RpcPacket {
					op: Op::Handshake,
					data: json!({ "v": 1, "client_id": client_id.to_string() }),
				})
				.await?;
			framed.next().await;

			let (tx, rx) = framed.split();
			let (in_tx, in_rx) = watch::channel(None);
			let (out_tx, out_rx) = mpsc::channel(32);

			spawn(Self::process_inbox(rx, out_tx.clone(), in_tx));
			spawn(Self::process_outbox(tx, out_rx));

			senders.insert(id, out_tx);
			receivers.insert(id, Mutex::new(in_rx));
		}

		Ok(Self {
			tx: senders,
			rx: receivers,
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
					receiver.send(Some(packet))?;
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

	pub async fn maybe_authenticate_all(
		&mut self,
		app: AppHandle,
		client_secret: &str,
	) -> AppResult<()> {
		for id in self.tx.keys().copied() {
			let token = app.store("auth")?.get(id.to_string());
			let mut token = match token {
				None => self.authenticate(id, client_secret).await?,
				Some(value) => serde_json::from_value(value)?,
			};

			if token.expires_at < Timestamp::now() {
				token = self.authenticate(id, client_secret).await?;
			} else if token.expires_at < Timestamp::now() + SignedDuration::from_hours(24) {
				token = self.refresh(id, token, client_secret).await?;
			}

			app.store("auth")?
				.set(id.to_string(), serde_json::to_value(token)?);
		}

		Ok(())
	}

	#[must_use]
	async fn refresh(
		&self,
		socket_id: u8,
		token: OAuth2Token,
		client_secret: &str,
	) -> AppResult<OAuth2Token> {
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
			socket_id,
			&Command {
				nonce: Ulid::new(),
				args: AuthenticateArgs {
					access_token: token.access_token.clone(),
				},
				cmd: "AUTHENTICATE",
			},
		)
		.await?;

		Ok(token)
	}

	#[must_use]
	pub async fn authenticate(&self, socket_id: u8, client_secret: &str) -> AppResult<OAuth2Token> {
		let nonce = Ulid::new();
		self.send(
			socket_id,
			&Command {
				nonce,
				args: AuthorizeArgs {
					client_id: self.client_id.to_string(),
					scopes: &["rpc", "rpc.activities.write"],
				},
				cmd: "AUTHORIZE",
			},
		)
		.await?;

		let res = self
			.expect_response::<AuthorizeData>(socket_id, Some(nonce))
			.await?;
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
		let token: OAuth2Token = res.into();

		self.send(
			socket_id,
			&Command {
				nonce: Ulid::new(),
				args: AuthenticateArgs {
					access_token: token.access_token.clone(),
				},
				cmd: "AUTHENTICATE",
			},
		)
		.await?;

		Ok(token)
	}

	pub async fn set_activity(&self, activity: Activity) -> AppResult<()> {
		self.send_all(&Command {
			nonce: Ulid::new(),
			args: SetActivityArgs {
				pid: process::id(),
				activity: Some(activity),
			},
			cmd: "SET_ACTIVITY",
		})
		.await
	}

	pub async fn clear_activity(&self) -> AppResult<()> {
		self.send_all(&Command {
			nonce: Ulid::new(),
			args: SetActivityArgs {
				pid: process::id(),
				activity: None,
			},
			cmd: "SET_ACTIVITY",
		})
		.await
	}

	async fn send<T: Serialize + Debug>(&self, socket_id: u8, data: T) -> AppResult<()> {
		self.tx[&socket_id]
			.send(RpcPacket {
				op: Op::Frame,
				data: serde_json::to_value(data)?,
			})
			.await?;

		Ok(())
	}

	#[tracing::instrument(skip(self), err, level = Level::DEBUG)]
	async fn send_all<T: Serialize + Debug>(&self, data: T) -> AppResult<()> {
		for tx in self.tx.values() {
			tx.send(RpcPacket {
				op: Op::Frame,
				data: serde_json::to_value(&data)?,
			})
			.await?;
		}

		Ok(())
	}

	#[tracing::instrument(skip(self), ret, err, level = Level::DEBUG)]
	pub async fn expect_response<T: DeserializeOwned + Debug>(
		&self,
		socket_id: u8,
		nonce: Option<Ulid>,
	) -> AppResult<Response<T>> {
		let mut watcher = self
			.rx
			.get(&socket_id)
			.ok_or(anyhow!("socket not found"))?
			.lock()
			.await;
		watcher.changed().await?;
		let value = watcher.borrow();

		let res = serde_json::from_value::<Response<T>>(value.as_ref().unwrap().data.clone())?;

		if let Some(expected) = nonce {
			if Some(expected) != res.nonce {
				return Err(
					anyhow!("expected nonce {}, received {:?}", expected, res.nonce).into(),
				);
			}
		}

		Ok(res)
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
pub struct Response<T> {
	pub nonce: Option<Ulid>,
	pub data: T,
	// pub cmd: String,
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
