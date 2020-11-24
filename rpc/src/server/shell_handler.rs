// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use bytes::buf::BufExt;
use hyper::{Body, Request};
use serde::Serialize;

use crypto::hash::{chain_id_to_b58_string, HashType};
use shell::shell_channel::BlockApplied;
use tezos_api::ffi::ProtocolRpcError;
use tezos_messages::ts_to_rfc3339;
use tezos_wrapper::service::{ProtocolError, ProtocolServiceError};

use crate::{
    empty,
    encoding::{
        base_types::*,
        monitor::BootstrapInfo,
    },
    make_json_response,
    make_json_stream_response,
    result_option_to_json_response,
    result_to_json_response,
    ServiceResult,
    services,
};
use crate::helpers::{create_rpc_request, parse_block_hash, parse_chain_id};
use crate::server::{HasSingleValue, HResult, Params, Query, RpcServiceEnvironment};
use crate::services::base_services;

#[derive(Serialize)]
pub struct ErrorMessage {
    error_type: String,
    message: String,
}

pub async fn bootstrapped(_: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> HResult {
    let state_read = env.state().read().unwrap();

    let bootstrap_info = match state_read.current_head().as_ref() {
        Some(current_head) => {
            let current_head: BlockApplied = current_head.clone();
            let block = HashType::BlockHash.bytes_to_string(&current_head.header().hash);
            let timestamp = ts_to_rfc3339(current_head.header().header.timestamp());
            BootstrapInfo::new(block.into(), TimeStamp::Rfc(timestamp))
        }
        None => BootstrapInfo::new(String::new().into(), TimeStamp::Integral(0))
    };

    make_json_response(&bootstrap_info)
}

pub async fn commit_hash(_: Request<Body>, _: Params, _: Query, _: RpcServiceEnvironment) -> HResult {
    let resp = &UniString::from(env!("GIT_HASH"));
    make_json_response(&resp)
}

pub async fn active_chains(_: Request<Body>, _: Params, _: Query, _: RpcServiceEnvironment) -> HResult {
    empty()
}

pub async fn protocols(_: Request<Body>, _: Params, _: Query, _: RpcServiceEnvironment) -> HResult {
    empty()
}

pub async fn valid_blocks(_: Request<Body>, _: Params, _: Query, _: RpcServiceEnvironment) -> HResult {
    empty()
}

pub async fn head_chain(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    make_json_stream_response(base_services::get_current_head_monitor_header(chain_id, env.state())?.unwrap())
}

pub async fn chains_block_id(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    use crate::encoding::chain::BlockInfo;
    result_option_to_json_response(
        base_services::get_full_block(
            &chain_id,
            &block_hash,
            env.persistent_storage(),
        ).map(|res| res.map(BlockInfo::from)),
        env.log(),
    )
}

pub async fn chains_block_id_header(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_option_to_json_response(
        base_services::get_block_header(
            chain_id,
            block_hash,
            env.persistent_storage(),
        ),
        env.log(),
    )
}

pub async fn chains_block_id_header_shell(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_option_to_json_response(
        base_services::get_block_shell_header(
            chain_id,
            block_hash,
            env.persistent_storage(),
        ),
        env.log(),
    )
}

pub async fn context_raw_bytes(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;
    let prefix = params.get_str("any");

    result_to_json_response(
        base_services::get_context_raw_bytes(
            &block_hash,
            prefix,
            &env,
        ),
        env.log(),
    )
}

pub async fn mempool_pending_operations(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    result_to_json_response(
        services::mempool_services::get_pending_operations(
            &chain_id,
            env.state(),
        ),
        env.log(),
    )
}

pub async fn inject_operation(req: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let operation_data_raw = hyper::body::aggregate(req).await?;
    let operation_data: String = serde_json::from_reader(&mut operation_data_raw.reader())?;

    let shell_channel = env.shell_channel();

    // TODO: TE-221 - add optional chain_id to params mapping
    let chain_id_param = "main";
    let chain_id = parse_chain_id(chain_id_param, &env)?;

    result_to_json_response(
        services::mempool_services::inject_operation(
            chain_id,
            &operation_data,
            &env,
            shell_channel,
        ),
        env.log(),
    )
}

pub async fn inject_block(req: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let body = hyper::body::to_bytes(req.into_body()).await?;
    let body = String::from_utf8(body.to_vec())?;

    let shell_channel = env.shell_channel();

    // TODO: TE-221 - add optional chain_id to params mapping
    let chain_id_param = "main";
    let chain_id = parse_chain_id(chain_id_param, &env)?;

    result_to_json_response(
        services::mempool_services::inject_block(
            chain_id,
            &body,
            &env,
            shell_channel,
        ),
        env.log(),
    )
}

pub async fn get_block_protocols(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        base_services::get_block_protocols(
            &chain_id,
            &block_hash,
            env.persistent_storage(),
        ),
        env.log(),
    )
}

pub async fn get_block_hash(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        Ok(HashType::BlockHash.bytes_to_string(&block_hash)),
        env.log(),
    )
}

pub async fn get_chain_id(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;

    result_to_json_response(
        Ok(chain_id_to_b58_string(&chain_id)),
        env.log(),
    )
}

pub async fn get_block_operation_hashes(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        base_services::get_block_operation_hashes(
            &chain_id,
            &block_hash,
            env.persistent_storage(),
        ),
        env.log(),
    )
}

pub async fn live_blocks(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        services::base_services::live_blocks(chain_id, block_hash, &env),
        env.log(),
    )
}

pub async fn preapply_operations(req: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id_param = params.get_str("chain_id").unwrap();
    let chain_id = parse_chain_id(chain_id_param, &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    let rpc_request = create_rpc_request(req).await?;

    result_to_json_response(
        services::protocol::preapply_operations(chain_id_param, chain_id, block_hash, rpc_request, &env),
        env.log(),
    )
}

pub async fn preapply_block(req: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id_param = params.get_str("chain_id").unwrap();
    let chain_id = parse_chain_id(chain_id_param, &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    let rpc_request = create_rpc_request(req).await?;

    // launcher - we need the error from preapply
    match services::protocol::preapply_block(chain_id_param, chain_id, block_hash, rpc_request, &env) {
        Ok(resp) => result_to_json_response(Ok(resp), env.log()),
        Err(e) => {
            if let Some(err) = e.as_fail().downcast_ref::<ProtocolServiceError>() {
                if let ProtocolServiceError::ProtocolError { reason: ProtocolError::ProtocolRpcError { reason: ProtocolRpcError::FailedToCallProtocolRpc(message) } } = err {
                    return make_json_response(&ErrorMessage {
                        error_type: "ocaml".to_string(),
                        message: message.to_string(),
                    });
                }
            }
            empty()
        }
    }
}

pub async fn node_version(_: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    result_to_json_response(
        base_services::get_node_version(env.network_version()),
        env.log(),
    )
}