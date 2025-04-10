use std::fmt::Display;

use serde::Serialize;

#[derive(Debug)]
pub struct AppError(anyhow::Error);
pub type AppResult<T, E = AppError> = std::result::Result<T, E>;

impl<E> From<E> for AppError
where
	E: Into<anyhow::Error>,
{
	fn from(value: E) -> Self {
		Self(value.into())
	}
}

impl Serialize for AppError {
	fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		serializer.serialize_str(&self.0.to_string())
	}
}

impl Display for AppError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}
