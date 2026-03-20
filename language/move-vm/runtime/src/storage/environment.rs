// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::VMConfig,
    native_functions::{NativeFunction, NativeFunctions},
    storage::verified_module_cache::VERIFIED_MODULES_CACHE,
};
use ambassador::delegatable_trait;
use bytes::Bytes;
use move_binary_format::{
    access::{ModuleAccess, ScriptAccess},
    errors::{Location, PartialVMError, VMResult},
    file_format::CompiledScript,
    CompiledModule,
};
use move_bytecode_verifier::dependencies;
use move_core_types::{
    account_address::AccountAddress,
    identifier::{IdentStr, Identifier},
    vm_status::StatusCode,
};
use move_vm_types::{
    code::ModuleBytesStorage, loaded_data::struct_name_indexing::StructNameIndexMap,
    module_id_interner::InternedModuleIdPool, ty_interner::InternedTypePool,
};
use std::sync::Arc;

/// Shared runtime environment for loader-v2 style storage and caches.
#[derive(Clone)]
pub struct RuntimeEnvironment {
    vm_config: VMConfig,
    #[allow(dead_code)]
    natives: NativeFunctions,
    struct_name_index_map: Arc<StructNameIndexMap>,
    ty_pool: Arc<InternedTypePool>,
    module_id_pool: Arc<InternedModuleIdPool>,
}

impl RuntimeEnvironment {
    pub fn new(
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
    ) -> Self {
        Self::new_with_config(natives, VMConfig::default())
    }

    pub fn new_with_config(
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
        vm_config: VMConfig,
    ) -> Self {
        let natives = NativeFunctions::new(natives)
            .unwrap_or_else(|e| panic!("Failed to create native functions: {}", e));
        Self {
            vm_config,
            natives,
            struct_name_index_map: Arc::new(StructNameIndexMap::empty()),
            ty_pool: Arc::new(InternedTypePool::new()),
            module_id_pool: Arc::new(InternedModuleIdPool::new()),
        }
    }

    pub(crate) fn new_with_shared_name_cache(
        natives: NativeFunctions,
        vm_config: VMConfig,
        struct_name_index_map: Arc<StructNameIndexMap>,
    ) -> Self {
        Self {
            vm_config,
            natives,
            struct_name_index_map,
            ty_pool: Arc::new(InternedTypePool::new()),
            module_id_pool: Arc::new(InternedModuleIdPool::new()),
        }
    }

    pub fn vm_config(&self) -> &VMConfig {
        &self.vm_config
    }

    #[allow(dead_code)]
    pub(crate) fn natives(&self) -> &NativeFunctions {
        &self.natives
    }

    pub(crate) fn set_natives(&mut self, natives: NativeFunctions) {
        self.natives = natives;
    }

    pub fn struct_name_index_map(&self) -> &StructNameIndexMap {
        &self.struct_name_index_map
    }

    pub(crate) fn struct_name_index_map_arc(&self) -> Arc<StructNameIndexMap> {
        self.struct_name_index_map.clone()
    }

    pub fn ty_pool(&self) -> &InternedTypePool {
        &self.ty_pool
    }

    pub fn module_id_pool(&self) -> &InternedModuleIdPool {
        &self.module_id_pool
    }

    pub fn deserialize_into_compiled_module(&self, bytes: &Bytes) -> VMResult<CompiledModule> {
        CompiledModule::deserialize_with_config(bytes, &self.vm_config.deserializer_config).map_err(
            |err| {
                let msg = format!("Deserialization error: {:?}", err);
                PartialVMError::new(StatusCode::CODE_DESERIALIZATION_ERROR)
                    .with_message(msg)
                    .finish(Location::Undefined)
            },
        )
    }

    pub fn deserialize_into_script(&self, serialized_script: &[u8]) -> VMResult<CompiledScript> {
        CompiledScript::deserialize_with_config(
            serialized_script,
            &self.vm_config.deserializer_config,
        )
        .map_err(|err| {
            let msg = format!("[VM] deserializer for script returned error: {:?}", err);
            PartialVMError::new(StatusCode::CODE_DESERIALIZATION_ERROR)
                .with_message(msg)
                .finish(Location::Script)
        })
    }

    pub(crate) fn build_locally_verified_script(
        &self,
        compiled_script: Arc<CompiledScript>,
    ) -> VMResult<LocallyVerifiedScript> {
        move_bytecode_verifier::verify_script_with_config(
            &self.vm_config.verifier_config,
            compiled_script.as_ref(),
        )?;
        Ok(LocallyVerifiedScript(compiled_script))
    }

    pub(crate) fn build_verified_script(
        &self,
        locally_verified_script: LocallyVerifiedScript,
        immediate_dependencies: &[Arc<crate::loader::Module>],
        script_hash: &[u8; 32],
    ) -> VMResult<crate::loader::Script> {
        dependencies::verify_script(
            locally_verified_script.0.as_ref(),
            immediate_dependencies
                .iter()
                .map(|module| module.as_ref().module()),
        )?;
        crate::loader::Script::new(
            locally_verified_script.0,
            script_hash,
            self.struct_name_index_map(),
        )
    }

    pub(crate) fn build_locally_verified_module(
        &self,
        compiled_module: Arc<CompiledModule>,
        module_size: usize,
        module_hash: &[u8; 32],
    ) -> VMResult<LocallyVerifiedModule> {
        if !VERIFIED_MODULES_CACHE.contains(module_hash, &self.vm_config.verifier_config) {
            move_bytecode_verifier::verify_module_with_config(
                &self.vm_config.verifier_config,
                compiled_module.as_ref(),
            )?;
            VERIFIED_MODULES_CACHE.put(*module_hash, &self.vm_config.verifier_config);
        }
        Ok(LocallyVerifiedModule(compiled_module, module_size))
    }

    pub(crate) fn build_verified_module_with_linking_checks(
        &self,
        locally_verified_module: LocallyVerifiedModule,
        immediate_dependencies: &[Arc<crate::loader::Module>],
    ) -> VMResult<crate::loader::Module> {
        dependencies::verify_module(
            locally_verified_module.0.as_ref(),
            immediate_dependencies
                .iter()
                .map(|module| module.as_ref().module()),
        )?;
        crate::loader::Module::new(
            self.natives(),
            locally_verified_module.0,
            self.struct_name_index_map(),
        )
        .map_err(|err| err.finish(Location::Undefined))
    }

    pub(crate) fn build_verified_module_skip_linking_checks(
        &self,
        locally_verified_module: LocallyVerifiedModule,
    ) -> VMResult<crate::loader::Module> {
        crate::loader::Module::new(
            self.natives(),
            locally_verified_module.0,
            self.struct_name_index_map(),
        )
        .map_err(|err| err.finish(Location::Undefined))
    }

    #[cfg(fuzzing)]
    pub(crate) fn build_verified_module_unchecked(
        &self,
        compiled_module: Arc<CompiledModule>,
        module_size: usize,
    ) -> VMResult<crate::loader::Module> {
        crate::loader::Module::new(
            self.natives(),
            module_size,
            compiled_module,
            self.struct_name_index_map(),
        )
        .map_err(|err| err.finish(Location::Undefined))
    }

    pub fn paranoid_check_module_address_and_name(
        &self,
        module: &CompiledModule,
        expected_address: &AccountAddress,
        expected_module_name: &IdentStr,
    ) -> VMResult<()> {
        if self.vm_config.paranoid_type_checks {
            let actual_address = module.self_addr();
            let actual_module_name = module.self_name();
            if expected_address != actual_address || expected_module_name != actual_module_name {
                let msg = format!(
                    "Expected module {}::{}, but got {}::{}",
                    expected_address, expected_module_name, actual_address, actual_module_name
                );
                return Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message(msg)
                        .finish(Location::Undefined),
                );
            }
        }
        Ok(())
    }
}

#[delegatable_trait]
pub trait WithRuntimeEnvironment {
    fn runtime_environment(&self) -> &RuntimeEnvironment;
}

impl WithRuntimeEnvironment for RuntimeEnvironment {
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        self
    }
}

pub struct RuntimeEnvironmentRef<'a, T> {
    runtime_environment: &'a RuntimeEnvironment,
    inner: &'a T,
}

impl<'a, T> RuntimeEnvironmentRef<'a, T> {
    pub fn new(runtime_environment: &'a RuntimeEnvironment, inner: &'a T) -> Self {
        Self {
            runtime_environment,
            inner,
        }
    }

    pub fn inner(&self) -> &T {
        self.inner
    }
}

impl<T> WithRuntimeEnvironment for RuntimeEnvironmentRef<'_, T> {
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        self.runtime_environment
    }
}

impl<T> ModuleBytesStorage for RuntimeEnvironmentRef<'_, T>
where
    T: ModuleBytesStorage,
{
    fn fetch_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Bytes>> {
        self.inner.fetch_module_bytes(address, module_name)
    }
}

pub(crate) struct LocallyVerifiedModule(Arc<CompiledModule>, usize);

impl LocallyVerifiedModule {
    pub(crate) fn immediate_dependencies_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&AccountAddress, &IdentStr)> {
        self.0.immediate_dependencies_iter()
    }
}

pub(crate) struct LocallyVerifiedScript(Arc<CompiledScript>);

impl LocallyVerifiedScript {
    pub(crate) fn immediate_dependencies_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&AccountAddress, &IdentStr)> {
        self.0.immediate_dependencies_iter()
    }
}
