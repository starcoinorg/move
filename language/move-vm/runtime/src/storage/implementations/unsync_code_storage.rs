// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::duplicated_attributes)]

use crate::{
    loader::{Module, Script},
    storage::{
        environment::{
            ambassador_impl_WithRuntimeEnvironment, RuntimeEnvironment, WithRuntimeEnvironment,
        },
        implementations::unsync_module_storage::{AsUnsyncModuleStorage, UnsyncModuleStorage},
        layout_cache::NoOpLayoutCache,
        module_storage::{ambassador_impl_ModuleStorage, ModuleStorage},
    },
};
use ambassador::Delegate;
use bytes::Bytes;
use move_binary_format::{errors::VMResult, file_format::CompiledScript, CompiledModule};
use move_core_types::{
    account_address::AccountAddress, identifier::IdentStr, language_storage::ModuleId,
};
use move_vm_types::code::{
    ambassador_impl_ScriptCache, Code, ModuleBytesStorage, ScriptCache, UnsyncScriptCache,
};
use std::sync::Arc;

#[derive(Delegate)]
#[delegate(
    WithRuntimeEnvironment,
    target = "module_storage",
    where = "M: ModuleStorage"
)]
#[delegate(ModuleStorage, target = "module_storage", where = "M: ModuleStorage")]
#[delegate(ScriptCache, target = "script_cache", where = "M: ModuleStorage")]
pub struct UnsyncCodeStorage<M> {
    script_cache: UnsyncScriptCache<[u8; 32], CompiledScript, Script>,
    module_storage: M,
}

impl<M> NoOpLayoutCache for UnsyncCodeStorage<M> {}

impl<M: ModuleStorage> UnsyncCodeStorage<M> {
    fn new(module_storage: M) -> Self {
        Self {
            script_cache: UnsyncScriptCache::empty(),
            module_storage,
        }
    }

    pub fn module_storage(&self) -> &M {
        &self.module_storage
    }

    pub fn into_module_storage(self) -> M {
        self.module_storage
    }

    pub fn get_verified_script(&self, hash: &[u8; 32]) -> Option<Arc<Script>> {
        match self.script_cache.get_script(hash)? {
            Code::Verified(script) => Some(script),
            Code::Deserialized(_) => None,
        }
    }
}

pub trait AsUnsyncCodeStorage<'ctx, Ctx: ModuleBytesStorage + WithRuntimeEnvironment> {
    fn as_unsync_code_storage(&'ctx self) -> UnsyncCodeStorage<UnsyncModuleStorage<'ctx, Ctx>>;

    fn into_unsync_code_storage(self) -> UnsyncCodeStorage<UnsyncModuleStorage<'ctx, Ctx>>;
}

impl<'ctx, Ctx> AsUnsyncCodeStorage<'ctx, Ctx> for Ctx
where
    Ctx: ModuleBytesStorage + WithRuntimeEnvironment,
{
    fn as_unsync_code_storage(&'ctx self) -> UnsyncCodeStorage<UnsyncModuleStorage<'ctx, Ctx>> {
        UnsyncCodeStorage::new(self.as_unsync_module_storage())
    }

    fn into_unsync_code_storage(self) -> UnsyncCodeStorage<UnsyncModuleStorage<'ctx, Ctx>> {
        UnsyncCodeStorage::new(self.into_unsync_module_storage())
    }
}
