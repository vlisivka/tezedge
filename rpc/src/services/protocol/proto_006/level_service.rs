// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::convert::TryFrom;

use tezos_messages::p2p::binary_message::BinaryMessage;

use crate::helpers::ContextProtocolParam;

// TODO: remove after implemented rpc router
pub(crate) fn get_levels_in_current_cycle(context_proto_params: ContextProtocolParam, offset: Option<&str>) -> Result<(i32, i32), failure::Error> {
    let level = i32::try_from(context_proto_params.level)?;

    let offset = offset.unwrap_or("0").parse::<i32>()?;

    // deserialize constants
    let dynamic = tezos_messages::protocol::proto_006::constants::ParametricConstants::from_bytes(context_proto_params.constants_data)?;
    let blocks_per_cycle = dynamic.blocks_per_cycle();

    // cycle
    let cycle: i32 = level / blocks_per_cycle + offset;
    
    // return first and last level in a tuple
    Ok((cycle * blocks_per_cycle + 1, (cycle + 1) * blocks_per_cycle))
}