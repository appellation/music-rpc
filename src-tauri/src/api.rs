use blake3::hash;
use jiff::Timestamp;

#[derive(Debug, Clone)]
pub struct Api {
	pub base_url: &'static str,
	rq: reqwest::Client,
}

impl Api {
	pub fn new(api_url: &'static str) -> Self {
		Self {
			base_url: api_url,
			rq: reqwest::Client::new(),
		}
	}

	#[tracing::instrument(skip_all, err)]
	pub async fn set_artwork(
		&self,
		mime: String,
		bytes: Vec<u8>,
		expires_at: Timestamp,
	) -> anyhow::Result<()> {
		let hash = hash(&bytes);
		self.rq
			.put(format!("{}/{}", self.base_url, hash))
			.query(&[("expires_at", expires_at)])
			.header("content-type", mime)
			.body(bytes)
			.send()
			.await?
			.error_for_status()?;

		Ok(())
	}
}
