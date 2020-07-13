// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT

use assert_json_diff::assert_json_eq;
use serial_test::serial;

use crypto::hash::{ChainId, ProtocolHash};
use tezos_api::environment::{OPERATION_LIST_LIST_HASH_EMPTY, TEZOS_ENV, TezosEnvironmentConfiguration};
use tezos_api::ffi::{FfiRpcService, InitProtocolContextResult, JsonRpcRequest, ProtocolJsonRpcRequest, TezosRuntimeConfiguration};
use tezos_client::client;
use tezos_messages::p2p::binary_message::BinaryMessage;
use tezos_messages::p2p::encoding::prelude::*;

mod common;

fn init_test_runtime() {
    // init runtime and turn on/off ocaml logging
    client::change_runtime_configuration(
        TezosRuntimeConfiguration {
            log_enabled: common::is_ocaml_log_enabled(),
            no_of_ffi_calls_treshold_for_gc: common::no_of_ffi_calls_treshold_for_gc(),
            debug_mode: false,
        }
    ).unwrap();
}

fn init_test_protocol_context(dir_name: &str) -> (ChainId, BlockHeader, ProtocolHash, InitProtocolContextResult) {
    let tezos_env: &TezosEnvironmentConfiguration = TEZOS_ENV.get(&test_data::TEZOS_NETWORK).expect("no tezos environment configured");

    let result = client::init_protocol_context(
        common::prepare_empty_dir(dir_name),
        tezos_env.genesis.clone(),
        tezos_env.protocol_overrides.clone(),
        true,
        false,
        false,
        Some(test_data::get_patch_context()),
    ).unwrap();

    let genesis_commit_hash = match result.clone().genesis_commit_hash {
        None => panic!("we needed commit_genesis and here should be result of it"),
        Some(cr) => cr
    };

    (
        tezos_env.main_chain_id().expect("invalid chain id"),
        tezos_env.genesis_header(
            genesis_commit_hash,
            OPERATION_LIST_LIST_HASH_EMPTY.clone(),
        ).expect("genesis header error"),
        tezos_env.genesis_protocol().expect("protocol_hash error"),
        result
    )
}

#[test]
#[serial]
fn test_run_operations() -> Result<(), failure::Error> {
    init_test_runtime();

    // init empty context for test
    let (chain_id, genesis_block_header, ..) = init_test_protocol_context("test_run_operations_storage_01");

    // apply block 1
    let last_block = apply_blocks_1(&chain_id, &genesis_block_header);

    // prepare encoded operation to send
    let request = test_data::OPERATION_JSON_RPC_REQUEST_LEVEL_2;

    // FFI call for run_operation
    let request = ProtocolJsonRpcRequest {
        block_header: last_block,
        chain_arg: "main".to_string(),
        chain_id: chain_id.clone(),
        request: JsonRpcRequest {
            context_path: "/chains/main/blocks/head/helpers/scripts/run_operation".to_string(),
            body: request.to_string(),
        },
        ffi_service: FfiRpcService::HelpersRunOperation,
    };
    let response = client::call_protocol_json_rpc(&request)?;

    // assert result json
    assert_json_eq!(
        serde_json::from_str(&response.body)?,
        serde_json::from_str(&test_data::RUN_OPERTION_RESULT)?,
    );

    Ok(())
}

#[test]
#[serial]
fn test_preapply_operations() -> Result<(), failure::Error> {
    init_test_runtime();

    // init empty context for test
    let (chain_id, genesis_block_header, ..) = init_test_protocol_context("test_preapply_operations_storage_02");

    // apply block 1
    let last_block = apply_blocks_1(&chain_id, &genesis_block_header);

    // prepare encoded request to send
    let request = test_data::NEXT_OPERATION_JSON_RPC_REQUEST_LEVEL_2;

    // FFI call for run_operation
    let request = ProtocolJsonRpcRequest {
        block_header: last_block,
        chain_arg: "main".to_string(),
        chain_id: chain_id.clone(),
        request: JsonRpcRequest {
            context_path: "/chains/main/blocks/head/helpers/preapply/operations".to_string(),
            body: request.to_string(),
        },
        ffi_service: FfiRpcService::HelpersPreapplyOperations,
    };
    let response = client::helpers_preapply_operations(&request)?;

    // assert result json
    assert_json_eq!(
        serde_json::from_str(&response.body)?,
        serde_json::from_str(&test_data::PREAPPLY_OPERTIONS_RESULT)?,
    );

    Ok(())
}

fn apply_blocks_1(chain_id: &ChainId, genesis_block_header: &BlockHeader) -> BlockHeader {
    // apply first block - level 1
    let block_header = BlockHeader::from_bytes(hex::decode(test_data::BLOCK_HEADER_LEVEL_1).unwrap()).unwrap();
    let apply_block_result = client::apply_block(
        &chain_id,
        &block_header,
        &genesis_block_header,
        &test_data::block_operations_from_hex(
            test_data::BLOCK_HEADER_HASH_LEVEL_1,
            test_data::block_header_level1_operations(),
        ),
        0,
    ).unwrap();
    assert_eq!(test_data::context_hash(test_data::BLOCK_HEADER_LEVEL_1_CONTEXT_HASH), apply_block_result.context_hash);
    assert_eq!(1, apply_block_result.max_operations_ttl);

    block_header
}

mod test_data {
    use crypto::hash::{ContextHash, HashType};
    use tezos_api::environment::TezosEnvironment;
    use tezos_api::ffi::PatchContext;
    use tezos_messages::p2p::binary_message::BinaryMessage;
    use tezos_messages::p2p::encoding::prelude::*;

    pub const TEZOS_NETWORK: TezosEnvironment = TezosEnvironment::Sandbox;

    pub fn context_hash(hash: &str) -> ContextHash {
        HashType::ContextHash
            .string_to_bytes(hash)
            .unwrap()
    }

    // BMPtRJqFGQJRTfn8bXQR2grLE1M97XnUmG5vgjHMW7St1Wub7Cd
    pub const BLOCK_HEADER_HASH_LEVEL_1: &str = "cf764612821498403dd6bd71e2fcbde9ad2c50670113dadc15ec52d167c66b31";
    pub const BLOCK_HEADER_LEVEL_1: &str = include_str!("resources/sandbox_block_header_level1.bytes");
    pub const BLOCK_HEADER_LEVEL_1_CONTEXT_HASH: &str = "CoUozrLGjbckFtx3PNhCz1KjacPL8CDEfzZ1WQAruSZLpUgwN378";

    pub const PATCH_CONTEXT: &str = include_str!("../../../light_node/etc/tezedge_sandbox/sandbox-patch-context.json");

    pub fn block_header_level1_operations() -> Vec<Vec<String>> {
        vec![]
    }

    // this should be the operation hash with a zero signature and zero fee (this should be accepted by the run_operation ffi)
    pub const OPERATION_JSON_RPC_REQUEST_LEVEL_2: &str = r#"
    { "operation":
      { "branch": "BMHeg41nrycUtki947NTgD8pawcpndVpAPa5udqcG6Yf9oxzNm4",
        "contents":
          [ { "kind": "transaction",
              "source": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx", "fee": "0",
              "counter": "1", "gas_limit": "1040000",
              "storage_limit": "60000", "amount": "1000000",
              "destination": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN" } ],
        "signature":
          "edsigtXomBKi5CTRf5cjATJWSyaRvhfYNHqSUGrn4SdbYRcGwQrUGjzEfQDTuqHhuA8b2d8NarZjz8TRf65WkpQmo423BtomS8Q" },
    "chain_id": "NetXdQprcVkpaWU" }
    "#;

    pub const RUN_OPERTION_RESULT: &str = r#"
    { "contents":
      [ { "amount": "1000000", "counter": "1",
          "destination": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN", "fee": "0",
          "gas_limit": "1040000", "kind": "transaction",
          "metadata":
            { "balance_updates": [],
              "operation_result":
                { "balance_updates":
                    [ { "change": "-1000000",
                        "contract": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx",
                        "kind": "contract" },
                      { "change": "1000000",
                        "contract": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN",
                        "kind": "contract" } ], "consumed_gas": "10207",
                  "status": "applied" } },
          "source": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx",
          "storage_limit": "60000" } ] }
    "#;

    pub const NEXT_OPERATION_JSON_RPC_REQUEST_LEVEL_2: &str = r#"
    [ { "protocol": "PsCARTHAGazKbHtnKfLzQg3kms52kSRpgnDY982a9oYsSXRLQEb",
      "branch": "BMHeg41nrycUtki947NTgD8pawcpndVpAPa5udqcG6Yf9oxzNm4",
      "contents":
        [ { "kind": "transaction",
            "source": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx", "fee": "1281",
            "counter": "1", "gas_limit": "10307", "storage_limit": "0",
            "amount": "1000000",
            "destination": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN" } ],
      "signature":
        "edsigtZvjo7z3EFUqUvfugvPqd2C7da3pKmCmzwgD9WMgvXL2uXNwcHP1beMVYbya9Hy1QBBdSWTznTznQ7Hhfq5cUpoNdVkS1W" } ]
     "#;

    pub const PREAPPLY_OPERTIONS_RESULT: &str = r#"
    [ { "contents":
        [ { "amount": "1000000", "counter": "1",
            "destination": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN",
            "fee": "1281", "gas_limit": "10307", "kind": "transaction",
            "metadata":
              { "balance_updates":
                  [ { "change": "-1281",
                      "contract": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx",
                      "kind": "contract" },
                    { "category": "fees", "change": "1281", "cycle": 0,
                      "delegate": "tz1Ke2h7sDdakHJQh8WX4Z372du1KChsksyU",
                      "kind": "freezer" } ],
                "operation_result":
                  { "balance_updates":
                      [ { "change": "-1000000",
                          "contract": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx",
                          "kind": "contract" },
                        { "change": "1000000",
                          "contract": "tz1gjaF81ZRRvdzjobyfVNsAeSC6PScjfQwN",
                          "kind": "contract" } ], "consumed_gas": "10207",
                    "status": "applied" } },
            "source": "tz1KqTpEZ7Yob7QbPE4Hy4Wo8fHG8LhKxZSx",
            "storage_limit": "0" } ],
      "signature":
        "edsigtZvjo7z3EFUqUvfugvPqd2C7da3pKmCmzwgD9WMgvXL2uXNwcHP1beMVYbya9Hy1QBBdSWTznTznQ7Hhfq5cUpoNdVkS1W" } ]
    "#;

    pub fn block_operations_from_hex(block_hash: &str, hex_operations: Vec<Vec<String>>) -> Vec<Option<OperationsForBlocksMessage>> {
        hex_operations
            .into_iter()
            .map(|bo| {
                let ops = bo
                    .into_iter()
                    .map(|op| Operation::from_bytes(hex::decode(op).unwrap()).unwrap())
                    .collect();
                Some(OperationsForBlocksMessage::new(OperationsForBlock::new(hex::decode(block_hash).unwrap(), 4), Path::Op, ops))
            })
            .collect()
    }

    pub fn get_patch_context() -> PatchContext {
        PatchContext {
            key: "sandbox_parameter".to_string(),
            json: PATCH_CONTEXT.to_string(),
        }
    }
}