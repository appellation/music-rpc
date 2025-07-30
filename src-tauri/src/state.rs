use tokio::sync::Mutex;

use crate::rpc::Rpc;

pub type RpcState = Mutex<Option<Rpc>>;
