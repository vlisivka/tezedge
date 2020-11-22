// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT
#![feature(test)]
extern crate test;

use std::time::{Duration, Instant};

use lazy_static::lazy_static;

use shell::peer_manager::P2p;
use shell::PeerConnectionThreshold;
use storage::{BlockMetaStorage, BlockMetaStorageReader};
use storage::tests_common::TmpStorage;
use tezos_identity::Identity;
use tezos_messages::p2p::encoding::version::NetworkVersion;

mod common;
mod samples;

lazy_static! {
    pub static ref NETWORK_VERSION: NetworkVersion = NetworkVersion::new("TEST_CHAIN".to_string(), 0, 0);
    pub static ref NODE_P2P_PORT: u16 = 1234; // TODO: maybe some logic to verify and get free port
    pub static ref NODE_P2P_CFG: (P2p, NetworkVersion) = (
        P2p {
            listener_port: NODE_P2P_PORT.clone(),
            bootstrap_lookup_addresses: vec![],
            disable_bootstrap_lookup: true,
            disable_mempool: false,
            private_node: false,
            initial_peers: vec![],
            peer_threshold: PeerConnectionThreshold::new(0, 10),
        },
        NETWORK_VERSION.clone(),
    );
    pub static ref NODE_IDENTITY: Identity = tezos_identity::Identity::generate(0f64);
}

#[ignore]
#[test]
fn test_process_current_branch_on_level3_with_empty_storage() -> Result<(), failure::Error> {
    // logger
    let log_level = common::log_level();
    let log = common::create_logger(log_level.clone());

    let db = test_cases_data::current_branch_on_level_3::init_data(&log);

    // start node
    let node = common::infra::NodeInfrastructure::start(
        TmpStorage::create(common::prepare_empty_dir("__test_01"))?,
        &common::prepare_empty_dir("__test_01_context"),
        "test_process_current_branch_on_level3_with_empty_storage",
        &db.tezos_env,
        None,
        Some(NODE_P2P_CFG.clone()),
        NODE_IDENTITY.clone(),
        (log, log_level),
    )?;

    // wait for storage initialization to genesis
    node.wait_for_new_current_head("genesis", node.tezos_env.genesis_header_hash()?, (Duration::from_secs(5), Duration::from_millis(250)))?;

    // connect mocked node peer with test data set
    let clocks = Instant::now();
    let mocked_peer_node = test_node_peer::TestNodePeer::connect(
        "TEST_PEER_NODE",
        NODE_P2P_CFG.0.listener_port,
        NODE_P2P_CFG.1.clone(),
        tezos_identity::Identity::generate(0f64),
        node.log.clone(),
        &node.tokio_runtime,
        test_cases_data::current_branch_on_level_3::serve_data,
    );

    // wait for current head on level 3
    node.wait_for_new_current_head("3", db.block_hash(3)?, (Duration::from_secs(30), Duration::from_millis(750)))?;
    println!("\nProcessed [3] in {:?}!\n", clocks.elapsed());

    // check context stored for all blocks
    node.wait_for_context("ctx_1", db.context_hash(1)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("ctx_2", db.context_hash(2)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("ctx_3", db.context_hash(3)?, (Duration::from_secs(5), Duration::from_millis(150)))?;

    // stop nodes
    drop(node);
    drop(mocked_peer_node);

    Ok(())
}

#[ignore]
#[test]
fn test_process_reorg_with_different_current_branches_with_empty_storage() -> Result<(), failure::Error> {
    // logger
    let log_level = common::log_level();
    let log = common::create_logger(log_level.clone());

    // prepare env data
    let (tezos_env, patch_context) = {
        let (db, patch_context) = test_cases_data::sandbox_branch_1_level3::init_data(&log);
        (db.tezos_env, patch_context)
    };

    // start node
    let node = common::infra::NodeInfrastructure::start(
        TmpStorage::create(common::prepare_empty_dir("__test_02"))?,
        &common::prepare_empty_dir("__test_02_context"),
        "test_process_reorg_with_different_current_branches_with_empty_storage",
        &tezos_env,
        patch_context,
        Some(NODE_P2P_CFG.clone()),
        NODE_IDENTITY.clone(),
        (log, log_level),
    )?;

    // wait for storage initialization to genesis
    node.wait_for_new_current_head("genesis", node.tezos_env.genesis_header_hash()?, (Duration::from_secs(5), Duration::from_millis(250)))?;

    // connect mocked node peer with data for branch_1
    let (db_branch_1, ..) = test_cases_data::sandbox_branch_1_level3::init_data(&node.log);
    let clocks = Instant::now();
    let mocked_peer_node_branch_1 = test_node_peer::TestNodePeer::connect(
        "TEST_PEER_NODE_BRANCH_1",
        NODE_P2P_CFG.0.listener_port,
        NODE_P2P_CFG.1.clone(),
        tezos_identity::Identity::generate(0f64),
        node.log.clone(),
        &node.tokio_runtime,
        test_cases_data::sandbox_branch_1_level3::serve_data,
    );

    // wait for current head on level 3
    node.wait_for_new_current_head("branch1-3", db_branch_1.block_hash(3)?, (Duration::from_secs(30), Duration::from_millis(750)))?;
    println!("\nProcessed [branch1-3] in {:?}!\n", clocks.elapsed());

    drop(mocked_peer_node_branch_1);

    // connect mocked node peer with data for branch_2
    let clocks = Instant::now();
    let (db_branch_2, ..) = test_cases_data::sandbox_branch_2_level4::init_data(&node.log);
    let mocked_peer_node_branch_2 = test_node_peer::TestNodePeer::connect(
        "TEST_PEER_NODE_BRANCH_2",
        NODE_P2P_CFG.0.listener_port,
        NODE_P2P_CFG.1.clone(),
        tezos_identity::Identity::generate(0f64),
        node.log.clone(),
        &node.tokio_runtime,
        test_cases_data::sandbox_branch_2_level4::serve_data,
    );

    // wait for current head on level 4
    node.wait_for_new_current_head("branch2-4", db_branch_2.block_hash(4)?, (Duration::from_secs(30), Duration::from_millis(750)))?;
    println!("\nProcessed [branch2-4] in {:?}!\n", clocks.elapsed());


    ////////////////////////////////////////////
    // 1. CONTEXT - check context stored for all branches
    node.wait_for_context("db_branch_1_ctx_1", db_branch_1.context_hash(1)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("db_branch_1_ctx_2", db_branch_1.context_hash(2)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("db_branch_1_ctx_3", db_branch_1.context_hash(3)?, (Duration::from_secs(5), Duration::from_millis(150)))?;

    node.wait_for_context("db_branch_2_ctx_1", db_branch_2.context_hash(1)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("db_branch_2_ctx_2", db_branch_2.context_hash(2)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("db_branch_2_ctx_3", db_branch_2.context_hash(3)?, (Duration::from_secs(5), Duration::from_millis(150)))?;
    node.wait_for_context("db_branch_2_ctx_4", db_branch_2.context_hash(4)?, (Duration::from_secs(5), Duration::from_millis(150)))?;

    ////////////////////////////////////////////
    // 2. HISTORY of blocks - check live_blocks for both branches (kind of check by chain traversal throught predecessors)
    let genesis_block_hash = node.tezos_env.genesis_header_hash()?;
    let block_meta_storage = BlockMetaStorage::new(node.tmp_storage.storage());

    let live_blocks_branch_1 = block_meta_storage.get_live_blocks(db_branch_1.block_hash(3)?, 10)?;
    assert_eq!(4, live_blocks_branch_1.len());
    assert!(live_blocks_branch_1.contains(&genesis_block_hash));
    assert!(live_blocks_branch_1.contains(&db_branch_1.block_hash(1)?));
    assert!(live_blocks_branch_1.contains(&db_branch_1.block_hash(2)?));
    assert!(live_blocks_branch_1.contains(&db_branch_1.block_hash(3)?));

    let live_blocks_branch_2 = block_meta_storage.get_live_blocks(db_branch_2.block_hash(4)?, 10)?;
    assert_eq!(5, live_blocks_branch_2.len());
    assert!(live_blocks_branch_2.contains(&genesis_block_hash));
    assert!(live_blocks_branch_2.contains(&db_branch_2.block_hash(1)?));
    assert!(live_blocks_branch_2.contains(&db_branch_2.block_hash(2)?));
    assert!(live_blocks_branch_2.contains(&db_branch_2.block_hash(3)?));
    assert!(live_blocks_branch_2.contains(&db_branch_2.block_hash(4)?));

    // stop nodes
    drop(node);
    // drop(mocked_peer_node_branch_1);
    drop(mocked_peer_node_branch_2);

    Ok(())
}

/// Stored first cca first 1300 apply block data
mod test_data {
    use std::collections::HashMap;
    use std::convert::TryInto;

    use failure::format_err;

    use crypto::hash::{BlockHash, ContextHash};
    use tezos_api::environment::TezosEnvironment;
    use tezos_api::ffi::ApplyBlockRequest;
    use tezos_messages::p2p::binary_message::MessageHash;
    use tezos_messages::p2p::encoding::block_header::Level;
    use tezos_messages::p2p::encoding::prelude::{BlockHeader, OperationsForBlock, OperationsForBlocksMessage};

    use crate::samples::OperationsForBlocksMessageKey;

    pub struct Db {
        pub tezos_env: TezosEnvironment,
        requests: Vec<String>,
        headers: HashMap<BlockHash, (Level, ContextHash)>,
        operations: HashMap<OperationsForBlocksMessageKey, OperationsForBlocksMessage>,
    }

    impl Db {
        pub(crate) fn init_db((requests, operations, tezos_env): (Vec<String>, HashMap<OperationsForBlocksMessageKey, OperationsForBlocksMessage>, TezosEnvironment)) -> Db {
            let mut headers: HashMap<BlockHash, (Level, ContextHash)> = HashMap::new();

            // init headers
            for (idx, request) in requests.iter().enumerate() {
                let request = crate::samples::from_captured_bytes(request).expect("Failed to parse request");
                let block = request.block_header.message_hash().expect("Failed to decode message_hash");
                let context_hash: ContextHash = request.block_header.context().clone();
                headers.insert(block, (to_level(idx), context_hash));
            }

            Db {
                tezos_env,
                requests,
                headers,
                operations,
            }
        }

        pub fn get(&self, block_hash: &BlockHash) -> Result<Option<BlockHeader>, failure::Error> {
            match self.headers.get(block_hash) {
                Some((level, _)) => {
                    Ok(Some(self.captured_requests(*level)?.block_header))
                }
                None => Ok(None)
            }
        }

        pub fn get_operations_for_block(&self, block: &OperationsForBlock) -> Result<Option<OperationsForBlocksMessage>, failure::Error> {
            match self.operations.get(&OperationsForBlocksMessageKey::new(block.block_hash().clone(), block.validation_pass())) {
                Some(operations) => {
                    Ok(Some(operations.clone()))
                }
                None => Ok(None)
            }
        }

        pub fn block_hash(&self, searched_level: Level) -> Result<BlockHash, failure::Error> {
            let block_hash = self.headers
                .iter()
                .find(|(_, (level, _))| searched_level.eq(level))
                .map(|(k, _)| k.clone());
            match block_hash {
                Some(block_hash) => Ok(block_hash),
                None => Err(format_err!("No header found for level: {}", searched_level))
            }
        }

        pub fn context_hash(&self, searched_level: Level) -> Result<ContextHash, failure::Error> {
            let context_hash = self.headers
                .iter()
                .find(|(_, (level, _))| searched_level.eq(level))
                .map(|(_, (_, context_hash))| context_hash.clone());
            match context_hash {
                Some(context_hash) => Ok(context_hash),
                None => Err(format_err!("No header found for level: {}", searched_level))
            }
        }

        /// Create new struct from captured requests by level.
        fn captured_requests(&self, level: Level) -> Result<ApplyBlockRequest, failure::Error> {
            crate::samples::from_captured_bytes(&self.requests[to_index(level)])
        }
    }

    /// requests are indexed from 0, so [0] is level 1, [1] is level 2, and so on ...
    fn to_index(level: Level) -> usize {
        (level - 1).try_into().expect("Failed to convert level to usize")
    }

    fn to_level(idx: usize) -> Level {
        (idx + 1).try_into().expect("Failed to convert index to Level")
    }
}

/// Predefined data sets as callback functions for test node peer
mod test_cases_data {
    use std::{env, fs};
    use std::path::Path;
    use std::sync::Once;

    use lazy_static::lazy_static;
    use slog::{info, Logger};

    use tezos_api::ffi::PatchContext;
    use tezos_messages::p2p::encoding::block_header::Level;
    use tezos_messages::p2p::encoding::prelude::{BlockHeaderMessage, CurrentBranch, CurrentBranchMessage, PeerMessage, PeerMessageResponse};

    use crate::test_data::Db;

    lazy_static! {
        // prepared data - we have stored 1326 request for apply block + operations for CARTHAGENET
        pub static ref DB_1326_CARTHAGENET: Db = Db::init_db(
            crate::samples::read_data_apply_block_request_until_1326(),
        );
    }

    fn init_data_db_1326_carthagenet(log: &Logger) -> &'static Db {
        static INIT_DATA: Once = Once::new();
        INIT_DATA.call_once(|| {
            info!(log, "Initializing test data 1326_carthagenet...");
            let _ = DB_1326_CARTHAGENET.block_hash(1);
            info!(log, "Test data 1326_carthagenet initialized!");
        });
        &DB_1326_CARTHAGENET
    }

    pub mod current_branch_on_level_3 {
        use slog::Logger;

        use tezos_messages::p2p::encoding::prelude::PeerMessageResponse;

        use crate::test_cases_data::{full_data, init_data_db_1326_carthagenet};
        use crate::test_data::Db;

        pub fn init_data(log: &Logger) -> &'static Db {
            init_data_db_1326_carthagenet(log)
        }

        pub fn serve_data(message: PeerMessageResponse) -> Result<Vec<PeerMessageResponse>, failure::Error> {
            full_data(message, Some(3), &super::DB_1326_CARTHAGENET)
        }
    }

    pub mod sandbox_branch_1_level3 {
        use std::sync::Once;

        use lazy_static::lazy_static;
        use slog::{info, Logger};

        use tezos_api::environment::TezosEnvironment;
        use tezos_api::ffi::PatchContext;
        use tezos_messages::p2p::encoding::prelude::PeerMessageResponse;

        use crate::test_cases_data::full_data;
        use crate::test_data::Db;

        lazy_static! {
            pub static ref DB: Db = Db::init_db(
                crate::samples::read_data_zip("sandbox_branch_1_level3.zip", TezosEnvironment::Sandbox),
            );
        }

        pub fn init_data(log: &Logger) -> (&'static Db, Option<PatchContext>) {
            static INIT_DATA: Once = Once::new();
            INIT_DATA.call_once(|| {
                info!(log, "Initializing test data sandbox_branch_1_level3...");
                let _ = DB.block_hash(1);
                info!(log, "Test data sandbox_branch_1_level3 initialized!");
            });
            (&DB, Some(super::read_patch_context("sandbox-patch-context.json")))
        }

        pub fn serve_data(message: PeerMessageResponse) -> Result<Vec<PeerMessageResponse>, failure::Error> {
            full_data(message, Some(3), &DB)
        }
    }

    pub mod sandbox_branch_2_level4 {
        use std::sync::Once;

        use lazy_static::lazy_static;
        use slog::{info, Logger};

        use tezos_api::environment::TezosEnvironment;
        use tezos_api::ffi::PatchContext;
        use tezos_messages::p2p::encoding::prelude::PeerMessageResponse;

        use crate::test_cases_data::full_data;
        use crate::test_data::Db;

        lazy_static! {
            pub static ref DB: Db = Db::init_db(
                crate::samples::read_data_zip("sandbox_branch_2_level4.zip", TezosEnvironment::Sandbox),
            );
        }

        pub fn init_data(log: &Logger) -> (&'static Db, Option<PatchContext>) {
            static INIT_DATA: Once = Once::new();
            INIT_DATA.call_once(|| {
                info!(log, "Initializing test data sandbox_branch_2_level4...");
                let _ = DB.block_hash(1);
                info!(log, "Test data sandbox_branch_2_level4 initialized!");
            });
            (&DB, Some(super::read_patch_context("sandbox-patch-context.json")))
        }

        pub fn serve_data(message: PeerMessageResponse) -> Result<Vec<PeerMessageResponse>, failure::Error> {
            full_data(message, Some(4), &DB)
        }
    }

    fn read_patch_context(patch_context_json: &str) -> PatchContext {
        let path = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("tests")
            .join("resources")
            .join(patch_context_json);
        match fs::read_to_string(path) {
            | Ok(content) => PatchContext {
                key: "sandbox_parameter".to_string(),
                json: content,
            },
            | Err(e) => panic!("Cannot read file, reason: {:?}", e)
        }
    }

    fn full_data(message: PeerMessageResponse, desired_current_branch_level: Option<Level>, db: &Db) -> Result<Vec<PeerMessageResponse>, failure::Error> {
        match message.messages().get(0).unwrap() {
            PeerMessage::GetCurrentBranch(request) => {
                match desired_current_branch_level {
                    Some(level) => {
                        let block_hash = db.block_hash(level)?;
                        if let Some(block_header) = db.get(&block_hash)? {
                            let current_branch = CurrentBranchMessage::new(
                                request.chain_id.clone(),
                                CurrentBranch::new(
                                    block_header.clone(),
                                    vec![
                                        block_hash,
                                        block_header.predecessor().clone(),
                                    ],
                                ),
                            );
                            Ok(vec![current_branch.into()])
                        } else {
                            Ok(vec![])
                        }
                    }
                    None => Ok(vec![])
                }
            }
            PeerMessage::GetBlockHeaders(request) => {
                let mut responses: Vec<PeerMessageResponse> = Vec::new();
                for block_hash in request.get_block_headers() {
                    if let Some(block_header) = db.get(block_hash)? {
                        let msg: BlockHeaderMessage = block_header.into();
                        responses.push(msg.into());
                    }
                }
                Ok(responses)
            }
            PeerMessage::GetOperationsForBlocks(request) => {
                let mut responses: Vec<PeerMessageResponse> = Vec::new();
                for block in request.get_operations_for_blocks() {
                    if let Some(msg) = db.get_operations_for_block(block)? {
                        responses.push(msg.into());
                    }
                }
                Ok(responses)
            }
            _ => Ok(vec![])
        }
    }
}

/// Test node peer, which simulates p2p remote peer, communicates through real p2p socket
mod test_node_peer {
    use std::net::{Shutdown, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use slog::{crit, debug, error, info, Logger, warn};
    use tokio::net::TcpStream;
    use tokio::runtime::Runtime;
    use tokio::time::timeout;

    use networking::p2p::peer;
    use networking::p2p::peer::{Bootstrap, BootstrapOutput, Local};
    use tezos_identity::Identity;
    use tezos_messages::p2p::encoding::prelude::{PeerMessage, PeerMessageResponse};
    use tezos_messages::p2p::encoding::version::NetworkVersion;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
    const READ_TIMEOUT_LONG: Duration = Duration::from_secs(30);

    pub struct TestNodePeer {
        run: Arc<AtomicBool>,
    }

    impl TestNodePeer {
        pub fn connect(
            name: &'static str,
            connect_to_node_port: u16,
            network_version: NetworkVersion,
            identity: Identity,
            log: Logger,
            tokio_runtime: &Runtime,
            handle_message_callback: fn(PeerMessageResponse) -> Result<Vec<PeerMessageResponse>, failure::Error>) -> TestNodePeer {
            let server_address = format!("0.0.0.0:{}", connect_to_node_port).parse::<SocketAddr>().expect("Failed to parse server address");
            let tokio_executor = tokio_runtime.handle().clone();
            let run = Arc::new(AtomicBool::new(false));

            {
                let run = run.clone();
                tokio_executor.spawn(async move {
                    // init socket connection to server node
                    match timeout(CONNECT_TIMEOUT, TcpStream::connect(&server_address)).await {
                        Ok(Ok(stream)) => {
                            info!(log, "[{}] Connection successful", name; "ip" => server_address);

                            // authenticate
                            let local = Arc::new(Local::new(
                                1235,
                                identity.public_key,
                                identity.secret_key,
                                identity.proof_of_work_stamp,
                                network_version,
                            ));
                            let bootstrap = Bootstrap::outgoing(
                                stream,
                                server_address,
                                false,
                                false,
                            );

                            let bootstrap_result = peer::bootstrap(
                                bootstrap,
                                local,
                                &log,
                            ).await.expect(&format!("[{}] Failed to bootstrap", name));

                            // process messages
                            run.store(true, Ordering::Release);
                            Self::begin_process_incoming(name, bootstrap_result, run, log, server_address, handle_message_callback).await;
                        }
                        Ok(Err(e)) => {
                            error!(log, "[{}] Connection failed", name; "ip" => server_address, "reason" => format!("{:?}", e));
                        }
                        Err(_) => {
                            error!(log, "[{}] Connection timed out", name; "ip" => server_address);
                        }
                    }
                });
            }

            TestNodePeer {
                run,
            }
        }

        /// Start to process incoming data
        async fn begin_process_incoming(
            name: &str,
            bootstrap: BootstrapOutput,
            run: Arc<AtomicBool>,
            log: Logger,
            peer_address: SocketAddr,
            handle_message_callback: fn(PeerMessageResponse) -> Result<Vec<PeerMessageResponse>, failure::Error>) {
            info!(log, "[{}] Starting to accept messages", name; "ip" => format!("{:?}", &peer_address));
            let BootstrapOutput(mut rx, mut tx, ..) = bootstrap;

            while run.load(Ordering::Acquire) {
                match timeout(READ_TIMEOUT_LONG, rx.read_message::<PeerMessageResponse>()).await {
                    Ok(res) => match res {
                        Ok(msg) => {
                            let msg_type = msg_type(&msg);
                            info!(log, "[{}] Handle message", name; "ip" => format!("{:?}", &peer_address), "msg_type" => msg_type.clone());

                            // apply callback
                            match handle_message_callback(msg) {
                                Ok(responses) => {
                                    info!(log, "[{}] Message handled({})", name, !responses.is_empty(); "msg_type" => msg_type);
                                    for response in responses {
                                        // send back response
                                        tx.write_message(&response).await.expect(&format!("[{}] Failed to send message", name));
                                    };
                                }
                                Err(e) => error!(log, "[{}] Failed to handle message", name; "reason" => format!("{:?}", e), "msg_type" => msg_type)
                            }
                        }
                        Err(e) => {
                            crit!(log, "[{}] Failed to read peer message", name; "reason" => e);
                            break;
                        }
                    }
                    Err(_) => {
                        warn!(log, "[{}] Peer message read timed out", name; "secs" => READ_TIMEOUT_LONG.as_secs());
                        break;
                    }
                }
            }

            debug!(log, "[{}] Shutting down peer connection", name; "ip" => format!("{:?}", &peer_address));
            // let mut tx_lock = tx.lock().await;
            // if let Some(tx) = tx_lock.take() {
            let socket = rx.unsplit(tx);
            match socket.shutdown(Shutdown::Both) {
                Ok(()) => debug!(log, "[{}] Connection shutdown successful", name; "socket" => format!("{:?}", socket)),
                Err(err) => debug!(log, "[{}] Failed to shutdown connection", name; "err" => format!("{:?}", err), "socket" => format!("{:?}", socket)),
            }
            // }

            info!(log, "[{}] Stopped to accept messages", name; "ip" => format!("{:?}", &peer_address));
        }

        pub fn stop(&mut self) {
            self.run.store(false, Ordering::Release);
        }
    }

    impl Drop for TestNodePeer {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn msg_type(msg: &PeerMessageResponse) -> String {
        msg.messages()
            .iter()
            .map(|m| match m {
                PeerMessage::Disconnect => "Disconnect",
                PeerMessage::Advertise(_) => "Advertise",
                PeerMessage::SwapRequest(_) => "SwapRequest",
                PeerMessage::SwapAck(_) => "SwapAck",
                PeerMessage::Bootstrap => "Bootstrap",
                PeerMessage::GetCurrentBranch(_) => "GetCurrentBranch",
                PeerMessage::CurrentBranch(_) => "CurrentBranch",
                PeerMessage::Deactivate(_) => "Deactivate",
                PeerMessage::GetCurrentHead(_) => "GetCurrentHead",
                PeerMessage::CurrentHead(_) => "CurrentHead",
                PeerMessage::GetBlockHeaders(_) => "GetBlockHeaders",
                PeerMessage::BlockHeader(_) => "BlockHeader",
                PeerMessage::GetOperations(_) => "GetOperations",
                PeerMessage::Operation(_) => "Operation",
                PeerMessage::GetProtocols(_) => "GetProtocols",
                PeerMessage::Protocol(_) => "Protocol",
                PeerMessage::GetOperationHashesForBlocks(_) => "GetOperationHashesForBlocks",
                PeerMessage::OperationHashesForBlock(_) => "OperationHashesForBlock",
                PeerMessage::GetOperationsForBlocks(_) => "GetOperationsForBlocks",
                PeerMessage::OperationsForBlocks(_) => "OperationsForBlocks",
            })
            .collect::<Vec<&str>>()
            .join(",")
    }
}
