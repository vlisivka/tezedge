// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::sync::Arc;

use derive_builder::Builder;
use getset::{CopyGetters, Getters};
use serde::{Deserialize, Serialize};

use crypto::hash::{BlockHash, ContextHash};

use crate::{BlockHeaderWithHash, Direction, IteratorMode, StorageError};
use crate::persistent::{BincodeEncoded, CommitLogSchema, CommitLogWithSchema, KeyValueSchema, KeyValueStoreWithSchema, Location, PersistentStorage};

/// Store block header data in a key-value store and into commit log.
/// The value is first inserted into commit log, which returns a location of the newly inserted value.
/// That location is then stored as a value in the key-value store.
///
/// The assumption is that, if primary_index contains block_hash, then also commit_log contains header data
#[derive(Clone)]
pub struct BlockStorage {
    primary_index: BlockPrimaryIndex,
    by_level_index: BlockByLevelIndex,
    by_context_hash_index: BlockByContextHashIndex,
    clog: Arc<BlockStorageCommitLog>,
}

pub type BlockStorageCommitLog = dyn CommitLogWithSchema<BlockStorage> + Sync + Send;

#[derive(Clone, Builder, Getters, Serialize, Deserialize, Debug)]
pub struct BlockJsonData {
    #[get = "pub"]
    block_header_proto_json: String,
    #[get = "pub"]
    block_header_proto_metadata_json: String,
    #[get = "pub"]
    operations_proto_metadata_json: String,
}

#[derive(Clone, Builder, CopyGetters, Serialize, Deserialize, Debug)]
pub struct BlockAdditionalData {
    #[get_copy = "pub"]
    max_operations_ttl: u16,
    #[get_copy = "pub"]
    last_allowed_fork_level: i32,
}

pub trait BlockStorageReader: Sync + Send {
    fn get(&self, block_hash: &BlockHash) -> Result<Option<BlockHeaderWithHash>, StorageError>;

    fn get_location(&self, block_hash: &BlockHash) -> Result<Option<BlockStorageColumnsLocation>, StorageError>;

    fn get_with_json_data(&self, block_hash: &BlockHash) -> Result<Option<(BlockHeaderWithHash, BlockJsonData)>, StorageError>;

    fn get_with_additional_data(&self, block_hash: &BlockHash) -> Result<Option<(BlockHeaderWithHash, BlockAdditionalData)>, StorageError>;

    fn get_multiple_with_json_data(&self, block_hash: &BlockHash, limit: usize) -> Result<Vec<(BlockHeaderWithHash, BlockJsonData)>, StorageError>;

    fn get_multiple_without_json(&self, block_hash: &BlockHash, limit: usize) -> Result<Vec<BlockHeaderWithHash>, StorageError>;

    fn get_every_nth_with_json_data(&self, every_nth: BlockLevel, from_block_hash: &BlockHash, limit: usize) -> Result<Vec<(BlockHeaderWithHash, BlockJsonData)>, StorageError>;

    fn get_by_context_hash(&self, context_hash: &ContextHash) -> Result<Option<BlockHeaderWithHash>, StorageError>;

    fn get_by_block_level(&self, level: i32) -> Result<Option<BlockHeaderWithHash>, StorageError>;

    fn get_by_block_level_with_json_data(&self, level: BlockLevel) -> Result<Option<(BlockHeaderWithHash, BlockJsonData)>, StorageError>;

    fn contains(&self, block_hash: &BlockHash) -> Result<bool, StorageError>;
}

impl BlockStorage {
    pub fn new(persistent_storage: &PersistentStorage) -> Self {
        Self {
            primary_index: BlockPrimaryIndex::new(persistent_storage.kv()),
            by_level_index: BlockByLevelIndex::new(persistent_storage.kv()),
            by_context_hash_index: BlockByContextHashIndex::new(persistent_storage.kv()),
            clog: persistent_storage.clog(),
        }
    }

    /// Stores header in key-value store and commit_log.
    /// If called multiple times for the same header, data are stored just first time.
    /// Returns true, if it is a new block
    pub fn put_block_header(&self, block_header: &BlockHeaderWithHash) -> Result<bool, StorageError> {
        if self.primary_index.contains(&block_header.hash)? {
            // we assume that, if primary_index contains hash, then also commit_log contains header data, header data cannot be change, so there is nothing to do
            return Ok(false);
        }

        self.clog.append(&BlockStorageColumn::BlockHeader(block_header.clone()))
            .map_err(StorageError::from)
            .and_then(|block_header_location| {
                let location = BlockStorageColumnsLocation {
                    block_header: block_header_location,
                    block_json_data: None,
                    block_additional_data: None,
                };
                self.primary_index.put(&block_header.hash, &location)
                    .and(self.by_level_index.put(block_header.header.level(), &location))
                    .and(Ok(true))
            })
    }

    pub fn put_block_json_data(&self, block_hash: &BlockHash, json_data: BlockJsonData) -> Result<(), StorageError> {
        let updated_column_location = {
            let block_json_data_location = self.clog.append(&BlockStorageColumn::BlockJsonData(json_data))?;
            let mut column_location = self.primary_index.get(block_hash)?.ok_or(StorageError::MissingKey)?;
            column_location.block_json_data = Some(block_json_data_location);
            column_location
        };
        let block_header = self.get_block_header_by_location(&updated_column_location)?;
        // update indexes
        self.primary_index.put(&block_header.hash, &updated_column_location)
            .and(self.by_level_index.put(block_header.header.level(), &updated_column_location))
    }

    pub fn put_block_additional_data(&self, block_hash: &BlockHash, additional_data: BlockAdditionalData) -> Result<(), StorageError> {
        let updated_column_location = {
            let block_additional_data_location = self.clog.append(&BlockStorageColumn::BlockAdditionalData(additional_data))?;
            let mut column_location = self.primary_index.get(block_hash)?.ok_or(StorageError::MissingKey)?;
            column_location.block_additional_data = Some(block_additional_data_location);
            column_location
        };
        let block_header = self.get_block_header_by_location(&updated_column_location)?;
        // update indexes
        self.primary_index.put(&block_header.hash, &updated_column_location)
            .and(self.by_level_index.put(block_header.header.level(), &updated_column_location))
    }

    pub fn assign_to_context(&self, block_hash: &BlockHash, context_hash: &ContextHash) -> Result<(), StorageError> {
        match self.primary_index.get(block_hash)? {
            Some(location) => self.by_context_hash_index.put(context_hash, &location),
            None => Err(StorageError::MissingKey)
        }
    }

    #[inline]
    fn get_block_header_by_location(&self, location: &BlockStorageColumnsLocation) -> Result<BlockHeaderWithHash, StorageError> {
        match self.clog.get(&location.block_header).map_err(StorageError::from)? {
            BlockStorageColumn::BlockHeader(block_header) => Ok(block_header),
            _ => Err(StorageError::InvalidColumn)
        }
    }

    #[inline]
    fn get_block_json_data_by_location(&self, location: &BlockStorageColumnsLocation) -> Result<Option<BlockJsonData>, StorageError> {
        match &location.block_json_data {
            Some(block_json_data_location) => match self.clog.get(block_json_data_location).map_err(StorageError::from)? {
                BlockStorageColumn::BlockJsonData(json_data) => Ok(Some(json_data)),
                _ => Err(StorageError::InvalidColumn)
            }
            None => Ok(None)
        }
    }

    #[inline]
    fn get_block_additional_data_by_location(&self, location: &BlockStorageColumnsLocation) -> Result<Option<BlockAdditionalData>, StorageError> {
        match &location.block_additional_data {
            Some(block_additional_data_location) => match self.clog.get(block_additional_data_location).map_err(StorageError::from)? {
                BlockStorageColumn::BlockAdditionalData(data) => Ok(Some(data)),
                _ => Err(StorageError::InvalidColumn)
            }
            None => Ok(None)
        }
    }

    #[inline]
    fn get_blocks_with_json_data_by_location<I>(&self, locations: I) -> Result<Vec<(BlockHeaderWithHash, BlockJsonData)>, StorageError>
        where
            I: IntoIterator<Item=BlockStorageColumnsLocation>
    {
        locations
            .into_iter()
            .filter_map(|location| self.get_block_json_data_by_location(&location)
                .and_then(|json_data_opt| json_data_opt.map(|json_data| self.get_block_header_by_location(&location).map(|block_header| (block_header, json_data))).transpose()).transpose())
            .collect()
    }
}

impl BlockStorageReader for BlockStorage {
    #[inline]
    fn get(&self, block_hash: &BlockHash) -> Result<Option<BlockHeaderWithHash>, StorageError> {
        self.primary_index.get(block_hash)?
            .map(|location| self.get_block_header_by_location(&location))
            .transpose()
    }

    #[inline]
    fn get_location(&self, block_hash: &BlockHash) -> Result<Option<BlockStorageColumnsLocation>, StorageError> {
        self.primary_index.get(block_hash)
    }

    #[inline]
    fn get_with_json_data(&self, block_hash: &BlockHash) -> Result<Option<(BlockHeaderWithHash, BlockJsonData)>, StorageError> {
        match self.primary_index.get(block_hash)? {
            Some(location) => self.get_block_json_data_by_location(&location)?
                .map(|json_data| self.get_block_header_by_location(&location).map(|block_header| (block_header, json_data)))
                .transpose(),
            None => Ok(None)
        }
    }

    #[inline]
    fn get_with_additional_data(&self, block_hash: &BlockHash) -> Result<Option<(BlockHeaderWithHash, BlockAdditionalData)>, StorageError> {
        match self.primary_index.get(block_hash)? {
            Some(location) => self.get_block_additional_data_by_location(&location)?
                .map(|json_data| self.get_block_header_by_location(&location).map(|block_header| (block_header, json_data)))
                .transpose(),
            None => Ok(None)
        }
    }

    #[inline]
    fn get_multiple_with_json_data(&self, block_hash: &BlockHash, limit: usize) -> Result<Vec<(BlockHeaderWithHash, BlockJsonData)>, StorageError> {
        let locations = self.get(block_hash)?
            .map_or_else(|| Ok(Vec::new()), |block| self.by_level_index.get_blocks(block.header.level(), limit))?;
        self.get_blocks_with_json_data_by_location(locations)
    }

    #[inline]
    fn get_every_nth_with_json_data(&self, every_nth: BlockLevel, from_block_hash: &BlockHash, limit: usize) -> Result<Vec<(BlockHeaderWithHash, BlockJsonData)>, StorageError> {
        let locations = self.get(from_block_hash)?
            .map_or_else(|| Ok(Vec::new()), |block| self.by_level_index.get_blocks_by_nth_level(every_nth, block.header.level(), limit))?;
        self.get_blocks_with_json_data_by_location(locations)
    }

    #[inline]
    fn get_multiple_without_json(&self, block_hash: &BlockHash, limit: usize) -> Result<Vec<BlockHeaderWithHash>, StorageError> {
        self.get(block_hash)?
            .map_or_else(
                || Ok(Vec::new()),
                |block| self.by_level_index.get_blocks_directed(
                    block.header.level(), limit, Direction::Forward))?
            .into_iter()
            .map(|location| self.get_block_header_by_location(&location))
            .collect()
    }

    #[inline]
    fn get_by_context_hash(&self, context_hash: &ContextHash) -> Result<Option<BlockHeaderWithHash>, StorageError> {
        self.by_context_hash_index.get(context_hash)?
            .map(|location| self.get_block_header_by_location(&location))
            .transpose()
    }

    #[inline]
    fn get_by_block_level(&self, level: BlockLevel) -> Result<Option<BlockHeaderWithHash>, StorageError> {
        self.by_level_index.get(&level)?
            .map(|location| self.get_block_header_by_location(&location))
            .transpose()
    }

    #[inline]
    fn get_by_block_level_with_json_data(&self, level: BlockLevel) -> Result<Option<(BlockHeaderWithHash, BlockJsonData)>, StorageError> {
        match self.by_level_index.get(&level)? {
            Some(location) => self.get_block_json_data_by_location(&location)?
                .map(|json_data| self.get_block_header_by_location(&location).map(|block_header| (block_header, json_data)))
                .transpose(),
            None => Ok(None)
        }
    }

    #[inline]
    fn contains(&self, block_hash: &BlockHash) -> Result<bool, StorageError> {
        self.primary_index.contains(block_hash)
    }
}

impl CommitLogSchema for BlockStorage {
    type Value = BlockStorageColumn;

    #[inline]
    fn name() -> &'static str {
        "block_storage"
    }
}

/// This mimics columns in a classic relational database.
#[derive(Serialize, Deserialize)]
pub enum BlockStorageColumn {
    BlockHeader(BlockHeaderWithHash),
    BlockJsonData(BlockJsonData),
    BlockAdditionalData(BlockAdditionalData),
}

impl BincodeEncoded for BlockStorageColumn {}

/// Holds reference to all stored columns.
#[derive(Serialize, Deserialize, Debug)]
pub struct BlockStorageColumnsLocation {
    pub block_header: Location,
    pub block_json_data: Option<Location>,
    pub block_additional_data: Option<Location>,
}

impl BincodeEncoded for BlockStorageColumnsLocation {}


/// Index block data as `block_header_hash -> location`.
#[derive(Clone)]
pub struct BlockPrimaryIndex {
    kv: Arc<BlockPrimaryIndexKV>,
}

pub type BlockPrimaryIndexKV = dyn KeyValueStoreWithSchema<BlockPrimaryIndex> + Sync + Send;

impl BlockPrimaryIndex {
    fn new(kv: Arc<BlockPrimaryIndexKV>) -> Self {
        Self { kv }
    }

    #[inline]
    fn put(&self, block_hash: &BlockHash, location: &BlockStorageColumnsLocation) -> Result<(), StorageError> {
        self.kv.put(block_hash, &location)
            .map_err(StorageError::from)
    }

    #[inline]
    fn get(&self, block_hash: &BlockHash) -> Result<Option<BlockStorageColumnsLocation>, StorageError> {
        self.kv.get(block_hash)
            .map_err(StorageError::from)
    }

    #[inline]
    fn contains(&self, block_hash: &BlockHash) -> Result<bool, StorageError> {
        self.kv.contains(block_hash)
            .map_err(StorageError::from)
    }
}

impl KeyValueSchema for BlockPrimaryIndex {
    type Key = BlockHash;
    type Value = BlockStorageColumnsLocation;

    #[inline]
    fn name() -> &'static str {
        "block_storage"
    }
}

/// Index block data as `level -> location`.
#[derive(Clone)]
pub struct BlockByLevelIndex {
    kv: Arc<BlockByLevelIndexKV>,
}

pub type BlockByLevelIndexKV = dyn KeyValueStoreWithSchema<BlockByLevelIndex> + Sync + Send;
pub type BlockLevel = i32;

impl BlockByLevelIndex {
    fn new(kv: Arc<BlockByLevelIndexKV>) -> Self {
        Self { kv }
    }

    fn put(&self, level: BlockLevel, location: &BlockStorageColumnsLocation) -> Result<(), StorageError> {
        self.kv.put(&level, location)
            .map_err(StorageError::from)
    }

    fn get(&self, level: &BlockLevel) -> Result<Option<BlockStorageColumnsLocation>, StorageError> {
        self.kv.get(level).map_err(StorageError::from)
    }

    fn get_blocks(&self, from_level: BlockLevel, limit: usize) -> Result<Vec<BlockStorageColumnsLocation>, StorageError> {
        self.kv.iterator(IteratorMode::From(&from_level, Direction::Reverse))?
            .take(limit)
            .map(|(_, location)| location.map_err(StorageError::from))
            .collect()
    }

    fn get_blocks_directed(&self, from_level: BlockLevel, limit: usize, direction: Direction) -> Result<Vec<BlockStorageColumnsLocation>, StorageError> {
        self.kv.iterator(IteratorMode::From(&from_level, direction))?
            .take(limit)
            .map(|(_, location)| location.map_err(StorageError::from))
            .collect()
    }

    fn get_blocks_by_nth_level(&self, every_nth: BlockLevel, from_level: BlockLevel, limit: usize) -> Result<Vec<BlockStorageColumnsLocation>, StorageError> {
        self.kv.iterator(IteratorMode::From(&from_level, Direction::Reverse))?
            .filter(|(level, _)| *level.as_ref().unwrap() % every_nth == 0)
            .take(limit)
            .map(|(_, location)| location.map_err(StorageError::from))
            .collect()
    }
}

impl KeyValueSchema for BlockByLevelIndex {
    type Key = BlockLevel;
    type Value = BlockStorageColumnsLocation;

    #[inline]
    fn name() -> &'static str {
        "block_by_level_storage"
    }
}


/// Index block data as `level -> location`.
#[derive(Clone)]
pub struct BlockByContextHashIndex {
    kv: Arc<BlockByContextHashIndexKV>,
}

pub type BlockByContextHashIndexKV = dyn KeyValueStoreWithSchema<BlockByContextHashIndex> + Sync + Send;

impl BlockByContextHashIndex {
    fn new(kv: Arc<BlockByContextHashIndexKV>) -> Self {
        Self { kv }
    }

    fn put(&self, context_hash: &ContextHash, location: &BlockStorageColumnsLocation) -> Result<(), StorageError> {
        self.kv.put(context_hash, location)
            .map_err(StorageError::from)
    }

    fn get(&self, context_hash: &ContextHash) -> Result<Option<BlockStorageColumnsLocation>, StorageError> {
        self.kv.get(context_hash).map_err(StorageError::from)
    }
}

impl KeyValueSchema for BlockByContextHashIndex {
    type Key = ContextHash;
    type Value = BlockStorageColumnsLocation;

    #[inline]
    fn name() -> &'static str {
        "block_by_context_hash_storage"
    }
}


#[cfg(test)]
mod tests {
    use std::path::Path;

    use failure::Error;

    use crate::persistent::{DbConfiguration, open_kv};

    use super::*;

    #[test]
    fn block_storage_level_index_order() -> Result<(), Error> {
        use rocksdb::{Options, DB, Cache};

        let path = "__block_level_index_test";
        if Path::new(path).exists() {
            std::fs::remove_dir_all(path).unwrap();
        }

        let cache = Cache::new_lru_cache(32 * 1024 * 1024).unwrap();

        {
            let db = open_kv(path, vec![BlockByLevelIndex::descriptor(&cache)], &DbConfiguration::default()).unwrap();
            let index = BlockByLevelIndex::new(Arc::new(db));

            for i in vec![1161, 66441, 905, 66185, 649, 65929, 393, 65673] {
                index.put(i, &BlockStorageColumnsLocation { block_header: Location::new(i as u64), block_json_data: None, block_additional_data: None })?;
            }

            let res = index.get_blocks(649, 2)?.iter().map(|location| location.block_header.offset()).collect::<Vec<_>>();
            assert_eq!(vec![649, 393], res);
            let res = index.get_blocks(65673, 100)?.iter().map(|location| location.block_header.offset()).collect::<Vec<_>>();
            assert_eq!(vec![65673, 1161, 905, 649, 393], res);
        }
        Ok(assert!(DB::destroy(&Options::default(), path).is_ok()))
    }
}
