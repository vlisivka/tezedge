// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use hyper::{Body, Request};
use slog::warn;

use crate::{
    result_to_json_response,
    ServiceResult,
    services,
};
use crate::helpers::{create_rpc_request, parse_block_hash, parse_chain_id};
use crate::server::{HasSingleValue, Params, Query, RpcServiceEnvironment};

pub async fn context_constants(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        services::protocol::get_context_constants_just_for_rpc(
            &block_hash,
            &env,
        ),
        env.log(),
    )
}

pub async fn baking_rights(_: Request<Body>, params: Params, query: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    let max_priority = query.get_str("max_priority");
    let level = query.get_str("level");
    let delegate = query.get_str("delegate");
    let cycle = query.get_str("cycle");
    let has_all = query.contains_key("all");

    // list -> context, persistent, state odizolovat
    match services::protocol::check_and_get_baking_rights(block_hash, level, delegate, cycle, max_priority, has_all, &env) {
        Ok(Some(rights)) => result_to_json_response(Ok(Some(rights)), env.log()),
        Err(e) => { //pass error to response parser
            let res: Result<Option<String>, failure::Error> = Err(e);
            result_to_json_response(res, env.log())
        }
        _ => { //ignore other options from enum
            warn!(env.log(), "Wrong RpcResponseData format");
            let res: Result<Option<String>, failure::Error> = Ok(None);
            result_to_json_response(res, env.log())
        }
    }
}

pub async fn endorsing_rights(_: Request<Body>, params: Params, query: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    let level = query.get_str("level");
    let cycle = query.get_str("cycle");
    let delegate = query.get_str("delegate");
    let has_all = query.contains_key("all");

    // get RPC response and unpack it from RpcResponseData enum
    match services::protocol::check_and_get_endorsing_rights(block_hash, level, delegate, cycle, has_all, &env) {
        Ok(Some(rights)) => result_to_json_response(Ok(Some(rights)), env.log()),
        Err(e) => { //pass error to response parser
            let res: Result<Option<String>, failure::Error> = Err(e);
            result_to_json_response(res, env.log())
        }
        _ => { //ignore other options from enum
            warn!(env.log(), "Wrong RpcResponseData format");
            let res: Result<Option<String>, failure::Error> = Ok(None);
            result_to_json_response(res, env.log())
        }
    }
}

pub async fn votes_listings(_: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id = parse_chain_id(params.get_str("chain_id").unwrap(), &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    result_to_json_response(
        services::protocol::get_votes_listings(
            block_hash,
            &env,
        ),
        env.log(),
    )
}

pub async fn call_protocol_rpc(req: Request<Body>, params: Params, _: Query, env: RpcServiceEnvironment) -> ServiceResult {
    let chain_id_param = params.get_str("chain_id").unwrap();
    let chain_id = parse_chain_id(chain_id_param, &env)?;
    let block_hash = parse_block_hash(&chain_id, params.get_str("block_id").unwrap(), &env)?;

    let json_request = create_rpc_request(req).await?;

    result_to_json_response(
        services::protocol::call_protocol_rpc(chain_id_param, chain_id, block_hash, json_request, &env),
        env.log(),
    )
}
