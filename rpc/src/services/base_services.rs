// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use failure::bail;

use crypto::hash::{BlockHash, chain_id_to_b58_string, ChainId, HashType};
use shell::shell_channel::BlockApplied;
use storage::{BlockHeaderWithHash, BlockMetaStorage, BlockMetaStorageReader, BlockStorage, BlockStorageReader, context_key};
use storage::block_storage::BlockJsonData;
use storage::context::ContextApi;
use storage::merkle_storage::StringTree;
use storage::persistent::PersistentStorage;
use tezos_messages::p2p::encoding::version::NetworkVersion;

use crate::helpers::{BlockHeaderInfo, BlockHeaderShellInfo, FullBlockInfo, get_context_hash, MonitorHeadStream, NodeVersion, Protocols};
use crate::rpc_actor::RpcCollectedStateRef;
use crate::server::RpcServiceEnvironment;

pub type BlockOperations = Vec<String>;

/// Retrieve blocks from database.
pub(crate) fn get_blocks(chain_id: ChainId, block_hash: BlockHash, every_nth_level: Option<i32>, limit: usize, persistent_storage: &PersistentStorage) -> Result<Vec<FullBlockInfo>, failure::Error> {
    let block_storage = BlockStorage::new(persistent_storage);
    let blocks = match every_nth_level {
        Some(every_nth_level) => block_storage.get_every_nth_with_json_data(every_nth_level, &block_hash, limit),
        None => block_storage.get_multiple_with_json_data(&block_hash, limit),
    }?.into_iter().map(|(header, json_data)| map_header_and_json_to_full_block_info(header, json_data, &chain_id)).collect();
    Ok(blocks)
}

/// Get information about current head monitor header as a stream of Json strings
pub(crate) fn get_current_head_monitor_header(chain_id: ChainId, state: &RpcCollectedStateRef) -> Result<Option<MonitorHeadStream>, failure::Error> {
    // create and return the a new stream on rpc call
    Ok(Some(MonitorHeadStream {
        chain_id,
        state: state.clone(),
        last_polled_timestamp: None,
    }))
}

/// Get information about block
pub(crate) fn get_full_block(chain_id: &ChainId, block_hash: &BlockHash, persistent_storage: &PersistentStorage) -> Result<Option<FullBlockInfo>, failure::Error> {
    Ok(get_block_by_block_id(chain_id, block_hash, persistent_storage)?)
}

/// Get information about block header
pub(crate) fn get_block_header(chain_id: ChainId, block_hash: BlockHash, persistent_storage: &PersistentStorage) -> Result<Option<BlockHeaderInfo>, failure::Error> {
    let block_storage = BlockStorage::new(persistent_storage);
    let block = block_storage
        .get_with_json_data(&block_hash)?
        .map(|(header, json_data)| map_header_and_json_to_block_header_info(header, json_data, &chain_id));

    Ok(block)
}

/// Get information about block shell header
pub(crate) fn get_block_shell_header(chain_id: ChainId, block_hash: BlockHash, persistent_storage: &PersistentStorage) -> Result<Option<BlockHeaderShellInfo>, failure::Error> {
    let block_storage = BlockStorage::new(persistent_storage);
    let block = block_storage
        .get_with_json_data(&block_hash)?
        .map(|(header, json_data)| map_header_and_json_to_block_header_info(header, json_data, &chain_id).to_shell_header());

    Ok(block)
}

pub(crate) fn live_blocks(_: ChainId, block_hash: BlockHash, env: &RpcServiceEnvironment) -> Result<Vec<String>, failure::Error> {
    let persistent_storage = env.persistent_storage();

    let block_storage = BlockStorage::new(persistent_storage);
    let block_meta_storage = BlockMetaStorage::new(persistent_storage);

    // get max_ttl for requested block
    let max_ttl: usize = match block_storage.get_with_additional_data(&block_hash)? {
        Some((_, json_data)) => {
            json_data.max_operations_ttl().into()
        }
        None => bail!("Max_ttl not found for block id: {}", HashType::BlockHash.bytes_to_string(&block_hash))
    };

    // get live blocks
    let live_blocks = block_meta_storage
        .get_live_blocks(block_hash, max_ttl)?
        .iter()
        .map(|block| HashType::BlockHash.bytes_to_string(&block))
        .collect();

    Ok(live_blocks)
}

pub(crate) fn get_context_raw_bytes(
    block_hash: &BlockHash,
    prefix: Option<&str>,
    env: &RpcServiceEnvironment) -> Result<StringTree, failure::Error> {

    // we assume that root is at "/data"
    let mut key_prefix = context_key!("data");

    // clients may pass in a prefix (without /data) with elements containing slashes (expecting us to split)
    // we need to join with '/' and split again
    if let Some(prefix) = prefix {
        key_prefix.extend(prefix.split('/').map(|s| s.to_string()));
    };

    let ctx_hash = get_context_hash(block_hash, env)?;
    Ok(env.tezedge_context().get_context_tree_by_prefix(&ctx_hash, &key_prefix)?)
}

/// Extract the current_protocol and the next_protocol from the block metadata
pub(crate) fn get_block_protocols(chain_id: &ChainId, block_hash: &BlockHash, persistent_storage: &PersistentStorage) -> Result<Protocols, failure::Error> {
    if let Some(block_info) = get_block_by_block_id(chain_id, &block_hash, persistent_storage)? {
        Ok(Protocols::new(
            block_info.metadata["protocol"].to_string().replace("\"", ""),
            block_info.metadata["next_protocol"].to_string().replace("\"", ""),
        ))
    } else {
        bail!("Cannot retrieve protocols, block_hash {} not found!", HashType::BlockHash.bytes_to_string(block_hash))
    }
}

/// Returns the chain id for the requested chain
pub(crate) fn get_block_operation_hashes(chain_id: &ChainId, block_hash: &BlockHash, persistent_storage: &PersistentStorage) -> Result<Vec<BlockOperations>, failure::Error> {
    if let Some(block_info) = get_block_by_block_id(chain_id, block_hash, persistent_storage)? {
        let operations = block_info.operations.into_iter()
            .map(|op_group| op_group.into_iter()
                .map(|op| op["hash"].to_string().replace("\"", ""))
                .collect())
            .collect();
        Ok(operations)
    } else {
        bail!("Cannot retrieve operation hashes from block, block_hash {} not found!", HashType::BlockHash.bytes_to_string(block_hash))
    }
}

pub(crate) fn get_node_version(network_version: &NetworkVersion) -> Result<NodeVersion, failure::Error> {
    Ok(NodeVersion::new(network_version))
}

pub(crate) fn get_block_by_block_id(chain_id: &ChainId, block_hash: &BlockHash, persistent_storage: &PersistentStorage) -> Result<Option<FullBlockInfo>, failure::Error> {
    Ok(
        BlockStorage::new(persistent_storage)
            .get_with_json_data(&block_hash)?
            .map(|(header, json_data)| map_header_and_json_to_full_block_info(header, json_data, &chain_id))
    )
}

#[inline]
fn map_header_and_json_to_full_block_info(header: BlockHeaderWithHash, json_data: BlockJsonData, chain_id: &ChainId) -> FullBlockInfo {
    let chain_id = chain_id_to_b58_string(chain_id);
    FullBlockInfo::new(&BlockApplied::new(header, json_data), chain_id)
}

#[inline]
fn map_header_and_json_to_block_header_info(header: BlockHeaderWithHash, json_data: BlockJsonData, chain_id: &ChainId) -> BlockHeaderInfo {
    let chain_id = chain_id_to_b58_string(chain_id);
    BlockHeaderInfo::new(&BlockApplied::new(header, json_data), chain_id)
}