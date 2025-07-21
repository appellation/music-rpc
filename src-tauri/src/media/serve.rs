use std::sync::Arc;

use axum::{
	extract::{Path, State},
	response::IntoResponse,
	routing::get,
	serve, Router,
};
use blake3::Hash;
use regex::bytes::Regex;
use reqwest::StatusCode;
use tauri::{async_runtime::spawn, AppHandle};
use tauri_plugin_shell::{process::CommandEvent, ShellExt};
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Clone)]
pub struct Server {
	current_artwork: Arc<Mutex<Option<Artwork>>>,
	pub public_url: Arc<Mutex<Option<String>>>,
}

impl Server {
	pub fn serve(app: AppHandle) -> anyhow::Result<Self> {
		let server = Self {
			current_artwork: Arc::default(),
			public_url: Arc::default(),
		};

		let router = Router::new()
			.route("/{hash}", get(Self::handle_request))
			.with_state(server.clone());

		let server2 = server.clone();
		spawn(async move {
			let listener = TcpListener::bind(("localhost", 0)).await.unwrap();
			spawn(server2.open_tunnel(app, listener.local_addr().unwrap().port()));
			serve(listener, router).await.unwrap();
		});

		Ok(server)
	}

	#[tracing::instrument(skip(self, bytes))]
	pub async fn set_artwork(&self, mime: String, bytes: Vec<u8>) {
		let mut artwork = self.current_artwork.lock().await;
		let hash = blake3::hash(&bytes);
		*artwork = Some(Artwork { bytes, mime, hash });
	}

	#[tracing::instrument(skip_all, err)]
	async fn handle_request(
		State(Server {
			current_artwork, ..
		}): State<Server>,
		Path(hash): Path<String>,
	) -> Result<impl IntoResponse, StatusCode> {
		let current_artwork = current_artwork.lock().await;
		let Some(artwork) = current_artwork.as_ref() else {
			return Err(StatusCode::NOT_FOUND);
		};

		let hash = Hash::from_hex(hash.as_bytes()).map_err(|_| StatusCode::NOT_FOUND)?;
		if hash == artwork.hash {
			tracing::info!(%hash, "serving image");

			Ok((
				StatusCode::OK,
				[("content-type", artwork.mime.clone())],
				artwork.bytes.clone(),
			))
		} else {
			Err(StatusCode::NOT_FOUND)
		}
	}

	#[tracing::instrument(skip(self, app))]
	async fn open_tunnel(self, app: AppHandle, port: u16) -> anyhow::Result<()> {
		let url_re = Regex::new("https://[a-zA-Z0-9-.]+\\.trycloudflare\\.com").unwrap();
		let (mut rx, _child) = app
			.shell()
			.sidecar("cloudflared")?
			.args(["tunnel", "--url", &format!("http://localhost:{port}")])
			.spawn()?;
		tracing::debug!("spawned cloudflared tunnel");

		while let Some(event) = rx.recv().await {
			if let CommandEvent::Stderr(bytes) = event {
				tracing::trace!(data = ?str::from_utf8(&bytes));

				if let Some(capture) = url_re.find(&bytes) {
					let url = String::from_utf8(capture.as_bytes().to_vec())?;
					tracing::info!("listening to port {} with url {}", port, url);

					let mut public_url = self.public_url.lock().await;
					*public_url = Some(url);
				}
			}
		}

		Ok(())
	}
}

struct Artwork {
	bytes: Vec<u8>,
	mime: String,
	hash: Hash,
}
