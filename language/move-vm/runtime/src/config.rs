// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::{
    deserializer::DeserializerConfig,
    file_format_common::{IDENTIFIER_SIZE_MAX, VERSION_MAX},
};
use move_bytecode_verifier::VerifierConfig;
use move_vm_types::loaded_data::runtime_types::TypeBuilder;
use serde::Serialize;

pub const DEFAULT_MAX_VALUE_NEST_DEPTH: u64 = 128;

/// Dynamic config options for the Move VM.
#[derive(Clone, Serialize)]
pub struct VMConfig {
    pub verifier_config: VerifierConfig,
    pub deserializer_config: DeserializerConfig,
    // When this flag is set to true, MoveVM will perform type checks at every instruction
    // execution to ensure that type safety cannot be violated at runtime.
    pub paranoid_type_checks: bool,
    pub check_invariant_in_swap_loc: bool,
    /// Maximum value nest depth for structs
    pub max_value_nest_depth: Option<u64>,
    pub type_max_cost: u64,
    pub type_base_cost: u64,
    pub type_byte_cost: u64,
    pub aggregator_v2_type_tagging: bool,
    pub ty_builder: TypeBuilder,
    pub layout_max_size: u64,
    pub layout_max_depth: u64,
    pub enable_function_caches: bool,
    pub enable_lazy_loading: bool,
    pub enable_depth_checks: bool,
    pub enable_layout_caches: bool,
}

impl Default for VMConfig {
    fn default() -> Self {
        Self {
            verifier_config: VerifierConfig::default(),
            deserializer_config: DeserializerConfig::new(VERSION_MAX, IDENTIFIER_SIZE_MAX),
            paranoid_type_checks: false,
            check_invariant_in_swap_loc: true,
            max_value_nest_depth: Some(DEFAULT_MAX_VALUE_NEST_DEPTH),
            type_max_cost: 0,
            type_base_cost: 0,
            type_byte_cost: 0,
            aggregator_v2_type_tagging: false,
            ty_builder: TypeBuilder::Legacy,
            layout_max_size: 512,
            layout_max_depth: 128,
            enable_function_caches: true,
            enable_lazy_loading: false,
            enable_depth_checks: true,
            enable_layout_caches: true,
        }
    }
}

impl VMConfig {
    pub fn production() -> Self {
        Self {
            verifier_config: VerifierConfig::production(),
            ..Self::default()
        }
    }
}
