use std::fmt::Display;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::{
	bytes::{Buf, BufMut},
	codec::{Decoder, Encoder},
};
use tracing::Level;

use crate::error::AppError;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Op {
	Handshake = 0,
	Frame = 1,
	Close = 2,
	Ping = 3,
	Pong = 4,
}

impl Display for Op {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}", self)
	}
}

impl TryFrom<i32> for Op {
	type Error = AppError;

	fn try_from(value: i32) -> Result<Self, Self::Error> {
		match value {
			v if v == Op::Handshake as i32 => Ok(Op::Handshake),
			v if v == Op::Frame as i32 => Ok(Op::Frame),
			v if v == Op::Close as i32 => Ok(Op::Close),
			v if v == Op::Ping as i32 => Ok(Op::Ping),
			v if v == Op::Pong as i32 => Ok(Op::Pong),
			other => Err(anyhow!("unexpected op code {}", other).into()),
		}
	}
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RpcPacket {
	pub op: Op,
	pub data: Value,
}

#[derive(Default)]
pub struct RpcCodec {
	op: Option<i32>,
	len: Option<usize>,
}

impl Decoder for RpcCodec {
	type Item = RpcPacket;
	type Error = AppError;

	#[tracing::instrument(skip_all, ret, err, level = Level::TRACE)]
	fn decode(
		&mut self,
		src: &mut tokio_util::bytes::BytesMut,
	) -> Result<Option<Self::Item>, Self::Error> {
		if src.len() < size_of::<i32>() * 2 {
			return Ok(None);
		}

		let op = *self.op.get_or_insert_with(|| src.get_i32_le());
		let len = *self.len.get_or_insert_with(|| src.get_i32_le() as usize);

		if src.remaining() < len {
			return Ok(None);
		}

		let buf = src.split_to(len);

		self.op = None;
		self.len = None;

		Ok(Some(RpcPacket {
			op: op.try_into()?,
			data: serde_json::from_slice(&buf)?,
		}))
	}
}

impl Encoder<RpcPacket> for RpcCodec {
	type Error = AppError;

	#[tracing::instrument(skip(self, dst), ret, err, level = Level::TRACE)]
	fn encode(
		&mut self,
		item: RpcPacket,
		dst: &mut tokio_util::bytes::BytesMut,
	) -> Result<(), Self::Error> {
		let buf = serde_json::to_vec(&item.data)?;

		dst.put_i32_le(item.op as i32);
		dst.put_i32_le(buf.len() as i32);
		dst.put(&*buf);

		Ok(())
	}
}
