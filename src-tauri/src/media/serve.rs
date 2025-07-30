use std::sync::Arc;

use axum::{
	Router,
	extract::{Path, State},
	http::StatusCode,
	response::IntoResponse,
	routing::get,
	serve,
};
use blake3::{Hash, hash};
use ngrok::{Session, config::ForwarderBuilder, forwarder::Forwarder, tunnel::HttpTunnel};
use tauri::async_runtime::spawn;
use tokio::{
	net::TcpListener,
	sync::{Mutex, watch},
};
use tracing::info;

#[derive(Clone)]
pub struct Server {
	current_artwork: Arc<Mutex<Option<Artwork>>>,
	url_ready: Arc<Mutex<watch::Receiver<bool>>>,
}

impl Server {
	pub fn serve() -> Self {
		let (ready_tx, ready_rx) = watch::channel(false);
		let server = Self {
			current_artwork: Arc::default(),
			url_ready: Arc::new(Mutex::new(ready_rx)),
		};

		let router = Router::new()
			.route("/{hash}", get(Self::handle_request))
			.with_state(server.clone());

		spawn(async move {
			let listener = TcpListener::bind(("localhost", 0)).await.unwrap();
			let _forwarder = Self::open_tunnel(listener.local_addr().unwrap().port())
				.await
				.unwrap();
			ready_tx.send(true).unwrap();
			info!("artwork server ready");
			serve(listener, router).await.unwrap();
		});

		server
	}

	#[tracing::instrument(skip(self, bytes))]
	pub async fn set_artwork(&self, mime: String, bytes: Vec<u8>) {
		let mut artwork = self.current_artwork.lock().await;
		let hash = hash(&bytes);
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

		let hash = hash.parse::<Hash>().map_err(|_| StatusCode::NOT_FOUND)?;
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

	#[tracing::instrument]
	async fn open_tunnel(port: u16) -> anyhow::Result<Forwarder<HttpTunnel>> {
		let session = Session::builder()
			.authtoken(env!("NGROK_AUTH_TOKEN"))
			.connect()
			.await?;

		Ok(session
			.http_endpoint()
			.domain(env!("NGROK_DOMAIN"))
			.pooling_enabled(true)
			.listen_and_forward(format!("http://localhost:{port}").parse()?)
			.await?)
	}

	pub async fn public_url(&self) -> &'static str {
		let mut ready = self.url_ready.lock().await;
		if !*ready.borrow() {
			ready.changed().await.unwrap();
		}

		env!("NGROK_DOMAIN")
	}
}

struct Artwork {
	bytes: Vec<u8>,
	mime: String,
	hash: Hash,
}
