// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::duplicated_attributes)]

use crate::{
    ambassador_impl_ModuleStorage, ambassador_impl_WithRuntimeEnvironment,
    loader::Module,
    storage::layout_cache::NoOpLayoutCache, AsUnsyncModuleStorage, ModuleStorage,
    RuntimeEnvironment, UnsyncModuleStorage, WithRuntimeEnvironment,
};
use ambassador::Delegate;
use bytes::Bytes;
use move_binary_format::{
    access::ModuleAccess,
    compatibility::Compatibility,
    errors::{verification_error, Location, PartialVMError, VMResult},
    normalized, CompiledModule, IndexKind,
};
use move_core_types::{
    account_address::AccountAddress,
    identifier::{IdentStr, Identifier},
    language_storage::ModuleId,
    vm_status::StatusCode,
};
use move_vm_types::{code::ModuleBytesStorage, module_linker_error, sha3_256};
use std::{
    collections::{btree_map, BTreeMap},
    sync::Arc,
};

pub struct VerifiedModuleBundle<K: Ord, V: Clone> {
    bundle: BTreeMap<K, V>,
}

impl<K: Ord, V: Clone> IntoIterator for VerifiedModuleBundle<K, V> {
    type IntoIter = btree_map::IntoIter<K, V>;
    type Item = (K, V);

    fn into_iter(self) -> Self::IntoIter {
        self.bundle.into_iter()
    }
}

struct StagingModuleBytesStorage<'a, M> {
    staged_runtime_environment: RuntimeEnvironment,
    staged_modules: BTreeMap<AccountAddress, BTreeMap<Identifier, (Bytes, Arc<CompiledModule>)>>,
    module_storage: &'a M,
}

impl<M> WithRuntimeEnvironment for StagingModuleBytesStorage<'_, M> {
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        &self.staged_runtime_environment
    }
}

impl<M: ModuleStorage> ModuleBytesStorage for StagingModuleBytesStorage<'_, M> {
    fn fetch_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Bytes>> {
        if let Some(account_storage) = self.staged_modules.get(address) {
            if let Some((bytes, _)) = account_storage.get(module_name) {
                return Ok(Some(bytes.clone()));
            }
        }
        self.module_storage
            .unmetered_get_module_bytes(address, module_name)
    }
}

#[derive(Delegate)]
#[delegate(WithRuntimeEnvironment, where = "M: ModuleStorage")]
#[delegate(ModuleStorage, where = "M: ModuleStorage")]
pub struct StagingModuleStorage<'a, M> {
    storage: UnsyncModuleStorage<'a, StagingModuleBytesStorage<'a, M>>,
}

impl<M> NoOpLayoutCache for StagingModuleStorage<'_, M> {}

impl<'a, M: ModuleStorage> StagingModuleStorage<'a, M> {
    pub fn create(
        sender: &AccountAddress,
        existing_module_storage: &'a M,
        module_bundle: Vec<Bytes>,
    ) -> VMResult<Self> {
        Self::create_with_compat_config(
            sender,
            Compatibility::full_check(),
            existing_module_storage,
            module_bundle,
        )
    }

    pub fn create_with_compat_config(
        sender: &AccountAddress,
        compatibility: Compatibility,
        existing_module_storage: &'a M,
        module_bundle: Vec<Bytes>,
    ) -> VMResult<Self> {
        let staged_runtime_environment = existing_module_storage.runtime_environment().clone();
        let is_lazy_loading_enabled = existing_module_storage
            .runtime_environment()
            .vm_config()
            .enable_lazy_loading;
        let deserializer_config = &staged_runtime_environment.vm_config().deserializer_config;

        let mut staged_modules = BTreeMap::new();
        for module_bytes in module_bundle {
            let compiled_module =
                CompiledModule::deserialize_with_config(&module_bytes, deserializer_config)
                    .map(Arc::new)
                    .map_err(|err| {
                        err.append_message_with_separator(
                            '\n',
                            "[VM] module deserialization failed".to_string(),
                        )
                        .finish(Location::Undefined)
                    })?;
            let addr = compiled_module.self_addr();
            let name = compiled_module.self_name();

            if addr != sender {
                let msg = format!(
                    "Compiled modules address {} does not match the sender {}",
                    addr, sender
                );
                return Err(verification_error(
                    StatusCode::MODULE_ADDRESS_DOES_NOT_MATCH_SENDER,
                    IndexKind::AddressIdentifier,
                    compiled_module.self_handle_idx().0,
                )
                .with_message(msg)
                .finish(Location::Undefined));
            }

            if compatibility.need_check_compat() {
                if let Some(old_module_ref) =
                    existing_module_storage.unmetered_get_deserialized_module(addr, name)?
                {
                    compatibility
                        .check(
                            &normalized::Module::new(old_module_ref.as_ref()),
                            &normalized::Module::new(compiled_module.as_ref()),
                        )
                        .map_err(|e| e.finish(Location::Undefined))?;
                }
            }

            use btree_map::Entry::*;
            let account_module_storage = match staged_modules.entry(*compiled_module.self_addr()) {
                Occupied(entry) => entry.into_mut(),
                Vacant(entry) => entry.insert(BTreeMap::new()),
            };
            let prev = account_module_storage.insert(
                compiled_module.self_name().to_owned(),
                (module_bytes, compiled_module.clone()),
            );

            if prev.is_some() {
                let msg = format!(
                    "Module {}::{} occurs more than once in published bundle",
                    compiled_module.self_addr(),
                    compiled_module.self_name()
                );
                return Err(PartialVMError::new(StatusCode::DUPLICATE_MODULE_NAME)
                    .with_message(msg)
                    .finish(Location::Undefined));
            }
        }

        let staged_module_bytes_storage = StagingModuleBytesStorage {
            staged_runtime_environment,
            staged_modules,
            module_storage: existing_module_storage,
        };

        let staged_module_storage = StagingModuleStorage {
            storage: staged_module_bytes_storage.into_unsync_module_storage(),
        };

        let staged_runtime_environment = staged_module_storage.runtime_environment();
        for (addr, name, bytes, compiled_module) in staged_module_storage
            .storage
            .byte_storage()
            .staged_modules
            .iter()
            .flat_map(|(addr, account_storage)| {
                account_storage
                    .iter()
                    .map(move |(name, (bytes, module))| (addr, name, bytes, module))
            })
        {
            if is_lazy_loading_enabled {
                staged_runtime_environment.paranoid_check_module_address_and_name(
                    compiled_module,
                    compiled_module.self_addr(),
                    compiled_module.self_name(),
                )?;
                let locally_verified_code = staged_runtime_environment
                    .build_locally_verified_module(
                        compiled_module.clone(),
                        bytes.len(),
                        &sha3_256(bytes),
                    )?;

                let mut verified_dependencies = vec![];
                for (dep_addr, dep_name) in locally_verified_code.immediate_dependencies_iter() {
                    let dependency =
                        staged_module_storage.unmetered_get_existing_lazily_verified_module(
                            &ModuleId::new(*dep_addr, dep_name.to_owned()),
                        )?;
                    verified_dependencies.push(dependency);
                }
                staged_runtime_environment.build_verified_module_with_linking_checks(
                    locally_verified_code,
                    &verified_dependencies,
                )?;
            } else {
                staged_module_storage
                    .unmetered_get_eagerly_verified_module(addr, name)?
                    .ok_or_else(|| {
                        PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                            .with_message(format!(
                                "Staged module {}::{} must always exist",
                                compiled_module.self_addr(),
                                compiled_module.self_name()
                            ))
                            .finish(Location::Undefined)
                    })?;
            }

            for (friend_addr, friend_name) in compiled_module.immediate_friends_iter() {
                if !staged_module_storage.unmetered_check_module_exists(friend_addr, friend_name)? {
                    return Err(module_linker_error!(friend_addr, friend_name));
                }
            }
        }

        Ok(staged_module_storage)
    }

    pub fn release_verified_module_bundle(self) -> VerifiedModuleBundle<ModuleId, Bytes> {
        let staged_modules = &self.storage.byte_storage().staged_modules;
        let mut bundle = BTreeMap::new();
        for (addr, account_storage) in staged_modules {
            for (name, (bytes, _)) in account_storage {
                bundle.insert(ModuleId::new(*addr, name.clone()), bytes.clone());
            }
        }
        VerifiedModuleBundle { bundle }
    }
}
