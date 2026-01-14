// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use bytes::Bytes;
use move_binary_format::{errors::PartialVMResult, file_format::CompiledModule};
use move_core_types::language_storage::ModuleId;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ModuleBytes {
    pub bytes: Bytes,
    pub size: usize,
}

impl ModuleBytes {
    pub fn new(bytes: Bytes) -> Self {
        let size = bytes.len();
        Self { bytes, size }
    }
}

#[derive(Clone, Debug)]
pub struct ModuleCode {
    pub module: Arc<CompiledModule>,
    pub size: usize,
    pub hash: Option<[u8; 32]>,
}

pub trait ModuleStorageV2 {
    fn module_bytes(&self, module_id: &ModuleId) -> PartialVMResult<Option<ModuleBytes>>;
    fn deserialized_module(&mut self, module_id: &ModuleId)
        -> PartialVMResult<Option<ModuleCode>>;
    fn verified_module(&self, module_id: &ModuleId) -> PartialVMResult<Option<ModuleCode>>;
}
