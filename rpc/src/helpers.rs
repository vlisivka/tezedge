// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::{collections::HashMap, convert::TryFrom};
use std::pin::Pin;

use chrono::Utc;
use failure::bail;
use futures::Stream;
use futures::task::{Context, Poll};
use hyper::{Body, Request};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crypto::hash::{BlockHash, chain_id_to_b58_string, ChainId, ContextHash, HashType};
use shell::shell_channel::BlockApplied;
use storage::{BlockMetaStorage, BlockMetaStorageReader, BlockStorage, BlockStorageReader, ChainMetaStorage};
use storage::chain_meta_storage::ChainMetaStorageReader;
use storage::context_action_storage::ContextActionType;
use tezos_api::ffi::{RpcMethod, RpcRequest};
use tezos_messages::p2p::encoding::block_header::Level;
use tezos_messages::p2p::encoding::prelude::*;
use tezos_messages::ts_to_rfc3339;

use crate::encoding::base_types::{TimeStamp, UniString};
use crate::rpc_actor::RpcCollectedStateRef;
use crate::server::RpcServiceEnvironment;

#[macro_export]
macro_rules! merge_slices {
    ( $($x:expr),* ) => {{
        let mut res = vec![];
        $(
            res.extend_from_slice($x);
        )*
        res
    }}
}

/// Object containing information to recreate the full block information
#[derive(Serialize, Debug, Clone)]
pub struct FullBlockInfo {
    pub hash: String,
    pub chain_id: String,
    pub header: InnerBlockHeader,
    pub metadata: HashMap<String, Value>,
    pub operations: Vec<Vec<HashMap<String, Value>>>,
}

/// Object containing all block header information
#[derive(Serialize, Debug, Clone)]
pub struct InnerBlockHeader {
    pub level: i32,
    pub proto: u8,
    pub predecessor: String,
    pub timestamp: String,
    pub validation_pass: u8,
    pub operations_hash: String,
    pub fitness: Vec<String>,
    pub context: String,
    pub protocol_data: HashMap<String, Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct HeaderContent {
    pub command: String,
    pub hash: String,
    pub fitness: Vec<String>,
    pub protocol_parameters: String,
}

/// Object containing information to recreate the block header information
#[derive(Serialize, Debug, Clone)]
pub struct BlockHeaderInfo {
    pub hash: String,
    pub chain_id: String,
    pub level: i32,
    pub proto: u8,
    pub predecessor: String,
    pub timestamp: String,
    pub validation_pass: u8,
    pub operations_hash: String,
    pub fitness: Vec<String>,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_nonce_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_of_work_nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<HeaderContent>,
}

/// Object containing information to recreate the block header shell information
#[derive(Serialize, Debug, Clone)]
pub struct BlockHeaderShellInfo {
    pub level: i32,
    pub proto: u8,
    pub predecessor: String,
    pub timestamp: String,
    pub validation_pass: u8,
    pub operations_hash: String,
    pub fitness: Vec<String>,
    pub context: String,
}

/// Object containing information to recreate the block header shell information
#[derive(Serialize, Debug, Clone)]
pub struct BlockHeaderMonitorInfo {
    pub hash: String,
    pub level: i32,
    pub proto: u8,
    pub predecessor: String,
    pub timestamp: String,
    pub validation_pass: u8,
    pub operations_hash: String,
    pub fitness: Vec<String>,
    pub context: String,
    pub protocol_data: String,
}

pub struct MonitorHeadStream {
    pub chain_id: ChainId,
    pub state: RpcCollectedStateRef,
    pub last_polled_timestamp: Option<TimeStamp>,
}

impl Stream for MonitorHeadStream {
    type Item = Result<String, serde_json::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<String, serde_json::Error>>> {
        // Note: the stream only ends on the client dropping the connection

        let state = self.state.read().unwrap();
        let last_update = if let TimeStamp::Integral(timestamp) = state.head_update_time() {
            *timestamp
        } else {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        };
        let current_head = state.current_head().clone();

        // drop the immutable borrow so we can borrow self again as mutable
        // TODO: refactor this drop (remove if possible)
        drop(state);

        if let Some(TimeStamp::Integral(poll_time)) = self.last_polled_timestamp {
            if poll_time < last_update {
                // get the desired structure of the
                let current_head = current_head.as_ref().map(|current_head| {
                    let chain_id = chain_id_to_b58_string(&self.chain_id);
                    BlockHeaderInfo::new(current_head, chain_id).to_monitor_header(current_head)
                });

                // serialize the struct to a json string to yield by the stream
                let mut head_string = serde_json::to_string(&current_head.unwrap())?;

                // push a newline character to the stream to imrove readability
                head_string.push('\n');

                self.last_polled_timestamp = Some(current_time_timestamp());

                // yield the serialized json
                return Poll::Ready(Some(Ok(head_string)));
            } else {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        } else {
            self.last_polled_timestamp = Some(current_time_timestamp());

            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

impl FullBlockInfo {
    pub fn new(val: &BlockApplied, chain_id: String) -> Self {
        let header: &BlockHeader = &val.header().header;
        let predecessor = HashType::BlockHash.bytes_to_string(header.predecessor());
        let timestamp = ts_to_rfc3339(header.timestamp());
        let operations_hash = HashType::OperationListListHash.bytes_to_string(header.operations_hash());
        let fitness = header.fitness().iter().map(|x| hex::encode(&x)).collect();
        let context = HashType::ContextHash.bytes_to_string(header.context());
        let hash = HashType::BlockHash.bytes_to_string(&val.header().hash);
        let json_data = val.json_data();

        Self {
            hash,
            chain_id,
            header: InnerBlockHeader {
                level: header.level(),
                proto: header.proto(),
                predecessor,
                timestamp,
                validation_pass: header.validation_pass(),
                operations_hash,
                fitness,
                context,
                protocol_data: serde_json::from_str(json_data.block_header_proto_json()).unwrap_or_default(),
            },
            metadata: serde_json::from_str(json_data.block_header_proto_metadata_json()).unwrap_or_default(),
            operations: serde_json::from_str(json_data.operations_proto_metadata_json()).unwrap_or_default(),
        }
    }
}

impl BlockHeaderInfo {
    pub fn new(val: &BlockApplied, chain_id: String) -> Self {
        let header: &BlockHeader = &val.header().header;
        let predecessor = HashType::BlockHash.bytes_to_string(header.predecessor());
        let timestamp = ts_to_rfc3339(header.timestamp());
        let operations_hash = HashType::OperationListListHash.bytes_to_string(header.operations_hash());
        let fitness = header.fitness().iter().map(|x| hex::encode(&x)).collect();
        let context = HashType::ContextHash.bytes_to_string(header.context());
        let hash = HashType::BlockHash.bytes_to_string(&val.header().hash);
        let header_data: HashMap<String, Value> = serde_json::from_str(val.json_data().block_header_proto_json()).unwrap_or_default();
        let signature = header_data.get("signature").map(|val| val.as_str().unwrap().to_string());
        let priority = header_data.get("priority").map(|val| val.as_i64().unwrap());
        let proof_of_work_nonce = header_data.get("proof_of_work_nonce").map(|val| val.as_str().unwrap().to_string());
        let seed_nonce_hash = header_data.get("seed_nonce_hash").map(|val| val.as_str().unwrap().to_string());
        let proto_data: HashMap<String, Value> = serde_json::from_str(val.json_data().block_header_proto_metadata_json()).unwrap_or_default();
        let protocol = proto_data.get("protocol").map(|val| val.as_str().unwrap().to_string());

        let mut content: Option<HeaderContent> = None;
        if let Some(header_content) = header_data.get("content") {
            content = serde_json::from_value(header_content.clone()).unwrap();
        }

        Self {
            hash,
            chain_id,
            level: header.level(),
            proto: header.proto(),
            predecessor,
            timestamp,
            validation_pass: header.validation_pass(),
            operations_hash,
            fitness,
            context,
            protocol,
            signature,
            priority,
            seed_nonce_hash,
            proof_of_work_nonce,
            content,
        }
    }

    pub fn to_shell_header(&self) -> BlockHeaderShellInfo {
        BlockHeaderShellInfo {
            level: self.level,
            proto: self.proto,
            predecessor: self.predecessor.clone(),
            timestamp: self.timestamp.clone(),
            validation_pass: self.validation_pass,
            operations_hash: self.operations_hash.clone(),
            fitness: self.fitness.clone(),
            context: self.context.clone(),
        }
    }

    pub fn to_monitor_header(&self, block: &BlockApplied) -> BlockHeaderMonitorInfo {
        BlockHeaderMonitorInfo {
            hash: self.hash.clone(),
            level: self.level,
            proto: self.proto,
            predecessor: self.predecessor.clone(),
            timestamp: self.timestamp.clone(),
            validation_pass: self.validation_pass,
            operations_hash: self.operations_hash.clone(),
            fitness: self.fitness.clone(),
            context: self.context.clone(),
            protocol_data: hex::encode(block.header().header.protocol_data()),
        }
    }
}

impl Into<HashMap<String, Value>> for InnerBlockHeader {
    fn into(self) -> HashMap<String, Value> {
        let mut map: HashMap<String, Value> = HashMap::new();
        map.insert("level".to_string(), self.level.into());
        map.insert("proto".to_string(), self.proto.into());
        map.insert("predecessor".to_string(), self.predecessor.into());
        map.insert("timestamp".to_string(), self.timestamp.into());
        map.insert("validation_pass".to_string(), self.validation_pass.into());
        map.insert("operations_hash".to_string(), self.operations_hash.into());
        map.insert("fitness".to_string(), self.fitness.into());
        map.insert("context".to_string(), self.context.into());
        map.extend(self.protocol_data);
        map
    }
}

/// Represents generic paged result.
#[derive(Debug, Serialize)]
pub struct PagedResult<C: Serialize> {
    /// Paged result data.
    data: C,
    /// ID of the next item if more items are available.
    /// If no more items are available then `None`.
    next_id: Option<u64>,
    /// Limit used in the request which produced this paged result.
    limit: usize,
}

#[allow(dead_code)]
impl<C> PagedResult<C>
    where
        C: Serialize
{
    pub fn new(data: C, next_id: Option<u64>, limit: usize) -> Self {
        PagedResult { data, next_id, limit }
    }
}

// TODO: refactor errors
/// Struct is defining Error message response, there are different keys is these messages so only needed one are defined for each message
#[derive(Serialize, Debug, Clone)]
pub struct RpcErrorMsg {
    kind: String,
    // "permanent"
    id: String,
    // "proto.005-PsBabyM1.seed.unknown_seed"
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_key: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oldest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requested: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct Protocols {
    protocol: String,
    next_protocol: String,
}

impl Protocols {
    pub fn new(protocol: String, next_protocol: String) -> Self {
        Self {
            protocol,
            next_protocol,
        }
    }
}

// ---------------------------------------------------------------------
#[derive(Serialize, Debug, Clone)]
pub struct NodeVersion {
    version: Version,
    network_version: NetworkVersion,
    commit_info: CommitInfo,
}

#[derive(Serialize, Debug, Clone)]
pub struct CommitInfo {
    commit_hash: UniString,
    commit_date: UniString,
}

#[derive(Serialize, Debug, Clone)]
pub struct Version {
    major: i32,
    minor: i32,
    additional_info: String,
}

impl NodeVersion {
    pub fn new(network_version: &NetworkVersion) -> Self {
        let version_env: &'static str = env!("CARGO_PKG_VERSION");

        let version: Vec<String> = version_env.split(".").map(|v| v.to_string()).collect();

        Self {
            version: Version {
                major: version[0].parse().unwrap_or(0),
                minor: version[1].parse().unwrap_or(0),
                additional_info: "release".to_string(),
            },
            network_version: network_version.clone(),
            commit_info: CommitInfo {
                commit_hash: UniString::from(env!("GIT_HASH")),
                commit_date: UniString::from(env!("GIT_COMMIT_DATE")),
            },
        }
    }
}

/// Parses [ChainId] from chain_id url param
pub(crate) fn parse_chain_id(chain_id_param: &str, env: &RpcServiceEnvironment) -> Result<ChainId, failure::Error> {
    Ok(
        match chain_id_param {
            "main" => env.main_chain_id().clone(),
            "test" => {
                // find test chain for main chain
                let chain_meta_storage = ChainMetaStorage::new(env.persistent_storage());
                let test_chain = match chain_meta_storage.get_test_chain_id(env.main_chain_id())? {
                    Some(test_chain_id) => test_chain_id,
                    None => bail!("No test chain activated for main_chain_id: {}", HashType::ChainId.bytes_to_string(env.main_chain_id()))
                };

                bail!("Test chains are not supported yet! main_chain_id: {}, test_chain_id: {}",
                    HashType::ChainId.bytes_to_string(env.main_chain_id()),
                    HashType::ChainId.bytes_to_string(&test_chain))
            }
            chain_id_hash => {
                let chain_id = HashType::ChainId.string_to_bytes(chain_id_hash)?;
                if chain_id.eq(env.main_chain_id()) {
                    chain_id
                } else {
                    bail!("Multiple chains are not supported yet! requested_chain_id: {} only main_chain_id: {}",
                        HashType::ChainId.bytes_to_string(&chain_id),
                        HashType::ChainId.bytes_to_string(env.main_chain_id()))
                }
            }
        }
    )
}

/// Parses [BlockHash] from block_id url param
/// # Arguments
///
/// * `block_id` - Url parameter block_id.
/// * `persistent_storage` - Persistent storage handler.
/// * `state` - Current RPC collected state (head).
///
/// `block_id` supports different formats:
/// - `head` - return current block_hash from RpcCollectedStateRef
/// - `genesis` - return genesis from RpcCollectedStateRef
/// - `<level>` - return block which is on the level according to actual current_head branch
/// - `<block_hash>` - return block hash directly
/// - `<block>~<level>` - block can be: genesis/head/level/block_hash, e.g.: head~10 returns: the block which is 10 levels in the past from head)
/// - `<block>-<level>` - block can be: genesis/head/level/block_hash, e.g.: head-10 returns: the block which is 10 levels in the past from head)
pub(crate) fn parse_block_hash(chain_id: &ChainId, block_id_param: &str, env: &RpcServiceEnvironment) -> Result<BlockHash, failure::Error> {
    // split header and optional offset (+, -, ~)
    let (block_param, offset_param) = {
        if block_id_param.contains('~') {
            let splits: Vec<&str> = block_id_param.split('~').collect();
            match splits.len() {
                1 => (splits[0], None),
                2 => (splits[0], Some(splits[1].parse::<i32>()?)),
                _ => bail!("Invalid block_id parameter: {}", block_id_param)
            }
        } else if block_id_param.contains('-') {
            let splits: Vec<&str> = block_id_param.split('~').collect();
            match splits.len() {
                1 => (splits[0], None),
                2 => (splits[0], Some(splits[1].parse::<i32>()?)),
                _ => bail!("Invalid block_id parameter: {}", block_id_param)
            }
        } else if block_id_param.contains('+') {
            let splits: Vec<&str> = block_id_param.split('~').collect();
            match splits.len() {
                1 => (splits[0], None),
                2 => (splits[0], Some(splits[1].parse::<i32>()? * -1)),
                _ => bail!("Invalid block_id parameter: {}", block_id_param)
            }
        } else {
            (block_id_param, None)
        }
    };

    // closure for current head
    let current_head = || {
        let state_read = env.state().read().unwrap();
        match state_read.current_head().as_ref() {
            Some(current_head) => Ok(
                (
                    current_head.header().hash.clone(),
                    current_head.header().header.level()
                )
            ),
            None => bail!("Head not initialized")
        }
    };

    let (block_hash, offset) = match block_param {
        "head" => {
            let (current_head, _) = current_head()?;
            if let Some(offset) = offset_param {
                if offset < 0 {
                    bail!("Offset for `head` parameter cannot be used with '+', block_id_param: {}", block_id_param);
                }
            }
            (current_head, offset_param)
        }
        "genesis" => {
            match ChainMetaStorage::new(env.persistent_storage()).get_genesis(chain_id)? {
                Some(genesis) => {
                    if let Some(offset) = offset_param {
                        if offset > 0 {
                            bail!("Offset for `genesis` parameter cannot be used with '~/-', block_id_param: {}", block_id_param);
                        }
                    }
                    (genesis.into(), offset_param)
                }
                None => bail!("No genesis found for chain_id: {}", HashType::ChainId.bytes_to_string(chain_id))
            }
        }
        level_or_hash => {
            // try to parse level as number
            match level_or_hash.parse::<Level>() {
                // block level was passed as parameter to block_id_param
                Ok(requested_level) => {
                    // we resolve level as relative to current_head - offset_to_level
                    let (current_head, current_head_level) = current_head()?;
                    let mut offset_from_head = current_head_level - requested_level;

                    // if we have also offset_param, we need to apply it
                    if let Some(offset) = offset_param {
                        offset_from_head -= offset;
                    }

                    // represet level as current_head with offset
                    (current_head, Some(offset_from_head))
                }
                Err(_) => {
                    // block hash as base58 string was passed as parameter to block_id
                    match HashType::BlockHash.string_to_bytes(level_or_hash) {
                        Ok(block_hash) => (block_hash, offset_param),
                        Err(e) => {
                            bail!("Invalid block_id_param: {}, reason: {}", block_id_param, e)
                        }
                    }
                }
            }
        }
    };

    // find requested header, if no offset we return header
    let block_hash = if let Some(offset) = offset {
        match BlockMetaStorage::new(env.persistent_storage())
            .find_block_at_distance(block_hash, offset)? {
            Some(block_hash) => block_hash,
            None => bail!("Unknown block for block_id_param: {}", block_id_param)
        }
    } else {
        block_hash
    };

    Ok(block_hash)
}

#[inline]
pub(crate) fn get_action_types(action_types: &str) -> Vec<ContextActionType> {
    action_types.split(",")
        .filter_map(|x: &str| x.parse().ok())
        .collect()
}

/// TODO: TE-238 - optimize context_hash/level index, not do deserialize whole header
/// TODO: returns context_hash and level, but level is here just for one use-case, so maybe it could be splitted
pub(crate) fn get_context_hash(block_hash: &BlockHash, env: &RpcServiceEnvironment) -> Result<ContextHash, failure::Error> {
    let block_storage = BlockStorage::new(env.persistent_storage());
    match block_storage.get(block_hash)? {
        Some(header) => Ok(header.header.context().clone()),
        None => bail!("Block not found for block_hash: {}", HashType::BlockHash.bytes_to_string(block_hash))
    }
}

pub(crate) fn current_time_timestamp() -> TimeStamp {
    TimeStamp::Integral(Utc::now().timestamp())
}

pub(crate) async fn create_rpc_request(req: Request<Body>) -> Result<RpcRequest, failure::Error> {
    let context_path = req.uri().path_and_query().unwrap().as_str().to_string();
    let meth = RpcMethod::try_from(req.method().to_string().as_str()).unwrap(); // TODO: handle correctly
    let content_type = match req.headers().get(hyper::header::CONTENT_TYPE) {
        None => None,
        Some(hv) => Some(String::from_utf8(hv.as_bytes().into())?),
    };
    let accept = match req.headers().get(hyper::header::ACCEPT) {
        None => None,
        Some(hv) => Some(String::from_utf8(hv.as_bytes().into())?),
    };
    let body = hyper::body::to_bytes(req.into_body()).await?;
    let body = String::from_utf8(body.to_vec())?;

    Ok(RpcRequest {
        body,
        context_path: String::from(context_path.trim_end_matches("/")),
        meth,
        content_type,
        accept,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: safe-guard in case `http` changes to decoding percent-encoding parts of the URI.
    // If that happens, update or remove this test and update the RPC-router in the OCaml
    // code so that it doesn't call `Uri.pct_decode` on the URI fragments. Code using the
    // query part of the URI may have to be updated too.
    #[test]
    fn test_pct_not_decoded() {
        let req = Request::builder()
            .uri("http://www.example.com/percent%20encoded?query=percent%20encoded")
            .body(())
            .unwrap();
        let path = req.uri().path_and_query().unwrap().as_str().to_string();
        let expected = "/percent%20encoded?query=percent%20encoded";
        assert_eq!(expected, &path);
    }
}