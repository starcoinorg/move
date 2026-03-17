// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    loader::Module, logging::expect_no_verification_errors, LayoutCache, WithRuntimeEnvironment,
};
use ambassador::delegatable_trait;
use bytes::Bytes;
use hashbrown::HashSet;
#[cfg(fuzzing)]
use move_binary_format::errors::Location;
use move_binary_format::{errors::VMResult, CompiledModule};
use move_core_types::{
    account_address::AccountAddress, identifier::IdentStr, language_storage::ModuleId,
};
use move_vm_types::{
    code::{ModuleCache, ModuleCode, ModuleCodeBuilder, WithBytes, WithHash, WithSize},
    module_cyclic_dependency_error, module_linker_error,
    value_serde::FunctionValueExtension,
};
use std::sync::Arc;

#[delegatable_trait]
pub trait ModuleStorage: WithRuntimeEnvironment + LayoutCache {
    fn unmetered_check_module_exists(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<bool>;

    fn unmetered_get_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Bytes>>;

    fn unmetered_get_module_size(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<usize>>;

    fn unmetered_get_existing_module_size(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<usize> {
        self.unmetered_get_module_size(address, module_name)?
            .ok_or_else(|| module_linker_error!(address, module_name))
    }

    fn unmetered_get_deserialized_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<CompiledModule>>>;

    fn unmetered_get_existing_deserialized_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Arc<CompiledModule>> {
        self.unmetered_get_deserialized_module(address, module_name)?
            .ok_or_else(|| module_linker_error!(address, module_name))
    }

    fn unmetered_get_eagerly_verified_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<Module>>>;

    fn unmetered_get_existing_eagerly_verified_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Arc<Module>> {
        self.unmetered_get_eagerly_verified_module(address, module_name)
            .map_err(expect_no_verification_errors)?
            .ok_or_else(|| module_linker_error!(address, module_name))
    }

    #[cfg(fuzzing)]
    fn unmetered_get_module_skip_verification(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<Module>>>;

    fn unmetered_get_lazily_verified_module(
        &self,
        module_id: &ModuleId,
    ) -> VMResult<Option<Arc<Module>>>;

    fn unmetered_get_existing_lazily_verified_module(
        &self,
        module_id: &ModuleId,
    ) -> VMResult<Arc<Module>> {
        self.unmetered_get_lazily_verified_module(module_id)
            .map_err(expect_no_verification_errors)?
            .ok_or_else(|| module_linker_error!(module_id.address(), module_id.name()))
    }
}

impl<T, E, V> ModuleStorage for T
where
    T: WithRuntimeEnvironment
        + LayoutCache
        + ModuleCache<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
            Version = V,
        > + ModuleCodeBuilder<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
        >,
    E: WithBytes + WithSize + WithHash,
    V: Clone + Default + Ord,
{
    fn unmetered_check_module_exists(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<bool> {
        let id = ModuleId::new(*address, module_name.to_owned());
        Ok(self.get_module_or_build_with(&id, self)?.is_some())
    }

    fn unmetered_get_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Bytes>> {
        let id = ModuleId::new(*address, module_name.to_owned());
        Ok(self
            .get_module_or_build_with(&id, self)?
            .map(|(module, _)| module.extension().bytes().clone()))
    }

    fn unmetered_get_module_size(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<usize>> {
        let id = ModuleId::new(*address, module_name.to_owned());
        Ok(self
            .get_module_or_build_with(&id, self)?
            .map(|(module, _)| module.extension().size_in_bytes()))
    }

    fn unmetered_get_deserialized_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<CompiledModule>>> {
        let id = ModuleId::new(*address, module_name.to_owned());
        Ok(self
            .get_module_or_build_with(&id, self)?
            .map(|(module, _)| module.code().deserialized().clone()))
    }

    fn unmetered_get_eagerly_verified_module(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<Module>>> {
        let id = ModuleId::new(*address, module_name.to_owned());
        let (module, version) = match self.get_module_or_build_with(&id, self)? {
            Some(module_and_version) => module_and_version,
            None => return Ok(None),
        };

        if module.code().is_verified() {
            return Ok(Some(module.code().verified().clone()));
        }

        let mut visited = HashSet::new();
        visited.insert(id.clone());
        Ok(Some(visit_dependencies_and_verify(
            id,
            module,
            version,
            &mut visited,
            self,
        )?))
    }

    #[cfg(fuzzing)]
    fn unmetered_get_module_skip_verification(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<Arc<Module>>> {
        let id = ModuleId::new(*address, module_name.to_owned());
        let (module, version) = match self.get_module_or_build_with(&id, self)? {
            Some(module_and_version) => module_and_version,
            None => return Ok(None),
        };
        if module.code().is_verified() {
            return Ok(Some(module.code().verified().clone()));
        }
        let mut visited = HashSet::new();
        visited.insert(id.clone());
        Ok(Some(visit_dependencies_and_skip_verification(
            id,
            module,
            version,
            &mut visited,
            self,
        )?))
    }

    fn unmetered_get_lazily_verified_module(
        &self,
        module_id: &ModuleId,
    ) -> VMResult<Option<Arc<Module>>> {
        let (module, version) = match self.get_module_or_build_with(module_id, self)? {
            Some(module_and_version) => module_and_version,
            None => return Ok(None),
        };

        if module.code().is_verified() {
            return Ok(Some(module.code().verified().clone()));
        }

        let runtime_environment = self.runtime_environment();
        runtime_environment.paranoid_check_module_address_and_name(
            module.code().deserialized(),
            module_id.address(),
            module_id.name(),
        )?;
        let locally_verified_code = runtime_environment.build_locally_verified_module(
            module.code().deserialized().clone(),
            module.extension().size_in_bytes(),
            module.extension().hash(),
        )?;
        let verified_code =
            runtime_environment.build_verified_module_skip_linking_checks(locally_verified_code)?;
        let verified_module = self.insert_verified_module(
            module_id.clone(),
            verified_code,
            module.extension().clone(),
            version,
        )?;
        Ok(Some(verified_module.code().verified().clone()))
    }
}

fn visit_dependencies_and_verify<T, E, V>(
    module_id: ModuleId,
    module: Arc<ModuleCode<CompiledModule, Module, E>>,
    version: V,
    visited: &mut HashSet<ModuleId>,
    module_cache_with_context: &T,
) -> VMResult<Arc<Module>>
where
    T: WithRuntimeEnvironment
        + ModuleCache<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
            Version = V,
        > + ModuleCodeBuilder<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
        >,
    E: WithBytes + WithSize + WithHash,
    V: Clone + Default + Ord,
{
    let runtime_environment = module_cache_with_context.runtime_environment();

    runtime_environment.paranoid_check_module_address_and_name(
        module.code().deserialized(),
        module_id.address(),
        module_id.name(),
    )?;
    let locally_verified_code = runtime_environment.build_locally_verified_module(
        module.code().deserialized().clone(),
        module.extension().size_in_bytes(),
        module.extension().hash(),
    )?;

    let mut verified_dependencies = vec![];
    for (addr, name) in locally_verified_code.immediate_dependencies_iter() {
        let dependency_id = ModuleId::new(*addr, name.to_owned());
        let (dependency, dependency_version) = module_cache_with_context
            .get_module_or_build_with(&dependency_id, module_cache_with_context)?
            .ok_or_else(|| module_linker_error!(addr, name))?;

        if dependency.code().is_verified() {
            verified_dependencies.push(dependency.code().verified().clone());
            continue;
        }

        if visited.insert(dependency_id.clone()) {
            let verified_dependency = visit_dependencies_and_verify(
                dependency_id.clone(),
                dependency,
                dependency_version,
                visited,
                module_cache_with_context,
            )?;
            verified_dependencies.push(verified_dependency);
        } else {
            return Err(module_cyclic_dependency_error!(
                dependency_id.address(),
                dependency_id.name()
            ));
        }
    }

    let verified_code = runtime_environment
        .build_verified_module_with_linking_checks(locally_verified_code, &verified_dependencies)?;
    let module = module_cache_with_context.insert_verified_module(
        module_id,
        verified_code,
        module.extension().clone(),
        version,
    )?;
    Ok(module.code().verified().clone())
}

#[cfg(fuzzing)]
fn visit_dependencies_and_skip_verification<T, E, V>(
    module_id: ModuleId,
    module: Arc<ModuleCode<CompiledModule, Module, E>>,
    version: V,
    visited: &mut HashSet<ModuleId>,
    module_cache_with_context: &T,
) -> VMResult<Arc<Module>>
where
    T: WithRuntimeEnvironment
        + ModuleCache<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
            Version = V,
        > + ModuleCodeBuilder<
            Key = ModuleId,
            Deserialized = CompiledModule,
            Verified = Module,
            Extension = E,
        >,
    E: WithBytes + WithSize + WithHash,
    V: Clone + Default + Ord,
{
    let runtime_environment = module_cache_with_context.runtime_environment();

    for (addr, name) in module.code().deserialized().immediate_dependencies_iter() {
        let dependency_id = ModuleId::new(*addr, name.to_owned());
        let (dependency, dependency_version) = module_cache_with_context
            .get_module_or_build_with(&dependency_id, module_cache_with_context)?
            .ok_or_else(|| module_linker_error!(addr, name))?;

        if dependency.code().is_verified() {
            continue;
        }

        if visited.insert(dependency_id.clone()) {
            let _ = visit_dependencies_and_skip_verification(
                dependency_id.clone(),
                dependency,
                dependency_version,
                visited,
                module_cache_with_context,
            )?;
        } else {
            return Err(module_cyclic_dependency_error!(
                dependency_id.address(),
                dependency_id.name()
            ));
        }
    }

    let code = runtime_environment.build_verified_module_unchecked(
        module.code().deserialized().clone(),
        module.extension().size_in_bytes(),
    )?;
    let verified_module = module_cache_with_context.insert_verified_module(
        module_id,
        code,
        module.extension().clone(),
        version,
    )?;
    Ok(verified_module.code().verified().clone())
}

pub struct FunctionValueExtensionAdapter<'a> {
    pub(crate) module_storage: &'a dyn ModuleStorage,
}

pub trait AsFunctionValueExtension {
    fn as_function_value_extension(&self) -> FunctionValueExtensionAdapter<'_>;
}

impl<T: ModuleStorage> AsFunctionValueExtension for T {
    fn as_function_value_extension(&self) -> FunctionValueExtensionAdapter<'_> {
        FunctionValueExtensionAdapter {
            module_storage: self,
        }
    }
}

impl FunctionValueExtension for FunctionValueExtensionAdapter<'_> {
    fn max_value_nest_depth(&self) -> Option<u64> {
        self.module_storage
            .runtime_environment()
            .vm_config()
            .max_value_nest_depth
    }
}
