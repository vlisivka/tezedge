// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use hyper::{Body, Request};
use slog::warn;

use crate::{empty, make_json_response, result_to_json_response, ServiceResult};
use crate::helpers::{parse_block_hash, parse_chain_id};
use crate::server::{HasSingleValue, Params, Query, RpcServiceEnvironment};
use crate::services::{base_services, dev_services};

pub async fn dev_blocks(_: Request<Body>, _: Params, query: Query, env: RpcServiceEnvironment) -> ServiceResult {
    // TODO: TE-221 - add optional chain_id to params mapping
    let chain_id_param = "main";
    let chain_id = parse_chain_id(chain_id_param, &env)?;

    // get block from params or fallback to current_head/genesis
    let from_block_id = match query.get_str("from_block_id") {
        Some(block_id_param) => parse_block_hash(&chain_id, block_id_param, &env)?,
        None => {
            // fallback, if no block param is present - check current head, if no one, then genesis
            let state = env.state().read().unwrap();
            match state.current_head() {
                Some(current_head) => current_head.header().hash.clone(),
                None => env.main_chain_genesis_hash().clone(),
            }
        }
    };

    // get cycle length
    let cycle_length = dev_services::get_cycle_length_for_block(&from_block_id, &env, env.log())?;
    let every_nth_level = match query.get_str("every_nth") {
        Some("cycle") => Some(cycle_length),
        Some("voting-period") => Some(cycle_length * 8),
        _ => None
    };
    let limit = query.get_usize("limit").unwrap_or(50);

    result_to_json_response(
        base_services::get_blocks(
            chain_id,
            from_block_id,
            every_nth_level,
            limit,
            env.persistent_storage(),
        ),
        env.log(),
    )
}

#[allow(dead_code)]
pub async fn dev_block_actions(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    // TODO: TE-221 - add optional chain_id to params mapping
    let chain_id_param = "main";
    let chain_id = parse_chain_id(chain_id_param, &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_hash").unwrap(), &env)?;
    result_to_json_response(
        dev_services::get_block_actions(
            block_hash,
            env.persistent_storage()),
        env.log(),
    )
}

#[allow(dead_code)]
pub async fn dev_contract_actions(_: Request<Body>, params: Params, query: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let contract_id = params.get_str("contract_address").unwrap();
    let from_id = query.get_u64("from_id");
    let limit = query.get_usize("limit").unwrap_or(50);
    result_to_json_response(dev_services::get_contract_actions(contract_id, from_id, limit, env.persistent_storage()), env.log())
}

pub async fn dev_action_cursor(_: Request<Body>, params: Params, query: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let cursor_id = query.get_u64("cursor_id");
    let limit = query.get_u64("limit").map(|limit| limit as usize);
    let action_types = query.get_str("action_types");
    result_to_json_response(if let Some(block_hash_param) = params.get_str("block_hash") {
        // TODO: TE-221 - add optional chain_id to params mapping
        let chain_id_param = "main";
        let chain_id = parse_chain_id(chain_id_param, &env)?;
        let block_hash = parse_block_hash(&chain_id, block_hash_param, &env)?;

        dev_services::get_block_actions_cursor(block_hash, cursor_id, limit, action_types, env.persistent_storage())
    } else if let Some(contract_address) = params.get_str("contract_address") {
        dev_services::get_contract_actions_cursor(contract_address, cursor_id, limit, action_types, env.persistent_storage())
    } else {
        unreachable!()
    }, env.log())
}

#[allow(dead_code)]
pub async fn dev_stats_storage(_: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    result_to_json_response(
        crate::services::stats_services::compute_storage_stats(
            env.state(),
            env.main_chain_genesis_hash(),
            env.persistent_storage()),
        env.log())
}

pub async fn dev_stats_memory(_: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    match dev_services::get_stats_memory() {
        Ok(resp) => make_json_response(&resp),
        Err(e) => {
            warn!(env.log(), "GetStatsMemory: {}", e);
            empty()
        }
    }
}

pub async fn database_memstats(_: Request<Body>, _: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    result_to_json_response(
        dev_services::get_database_memstats(env.tezedge_context()),
        env.log(),
    )
}