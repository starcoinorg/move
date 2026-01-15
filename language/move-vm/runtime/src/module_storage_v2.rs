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
    pub kind: ModuleCodeKind,
    pub version: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleCodeKind {
    Deserialized,
    Verified,
}

pub struct ModuleCodeBuilder {
    module: Arc<CompiledModule>,
    size: usize,
    hash: Option<[u8; 32]>,
    kind: ModuleCodeKind,
    version: Option<u64>,
}

impl ModuleCodeBuilder {
    pub fn deserialized(module: Arc<CompiledModule>, size: usize) -> Self {
        Self {
            module,
            size,
            hash: None,
            kind: ModuleCodeKind::Deserialized,
            version: None,
        }
    }

    pub fn verified(module: Arc<CompiledModule>, size: usize) -> Self {
        Self {
            module,
            size,
            hash: None,
            kind: ModuleCodeKind::Verified,
            version: None,
        }
    }

    pub fn with_hash(mut self, hash: [u8; 32]) -> Self {
        self.hash = Some(hash);
        self
    }

    pub fn with_version(mut self, version: u64) -> Self {
        self.version = Some(version);
        self
    }

    pub fn build(self) -> ModuleCode {
        ModuleCode {
            module: self.module,
            size: self.size,
            hash: self.hash,
            kind: self.kind,
            version: self.version,
        }
    }
}

pub trait ModuleStorageV2 {
    fn module_bytes(&self, module_id: &ModuleId) -> PartialVMResult<Option<ModuleBytes>>;
    fn deserialized_module(&mut self, module_id: &ModuleId)
        -> PartialVMResult<Option<ModuleCode>>;
    fn verified_module(&self, module_id: &ModuleId) -> PartialVMResult<Option<ModuleCode>>;
}
