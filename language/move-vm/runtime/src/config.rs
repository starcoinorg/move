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

/// Loader-related configuration toggles.
#[derive(Clone, Serialize)]
pub struct LoaderConfig {
    /// Enable lazy loading paths in the loader (disabled by default for loader-v1 behavior).
    pub enable_lazy_loading: bool,
    /// Enable layout caches used by lazy loading and type layout conversion.
    pub enable_layout_caches: bool,
    /// Enable script caches beyond the legacy loader-v1 behavior.
    pub enable_script_cache: bool,
}

impl Default for LoaderConfig {
    fn default() -> Self {
        Self {
            enable_lazy_loading: false,
            enable_layout_caches: false,
            enable_script_cache: false,
        }
    }
}

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
    /// Loader-related configuration toggles.
    pub loader_config: LoaderConfig,
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
            loader_config: LoaderConfig::default(),
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
