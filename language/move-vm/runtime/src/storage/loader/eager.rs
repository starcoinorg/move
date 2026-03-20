// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    check_dependencies_and_charge_gas,
    loader::{Function, Module, Script},
    module_traversal::TraversalContext,
    storage::{
        dependencies_gas_charging::check_type_tag_dependencies_and_charge_gas,
        loader::traits::{
            FunctionDefinitionLoader, InstantiatedFunctionLoader, InstantiatedFunctionLoaderHelper,
            LegacyLoaderConfig, Loader, ModuleMetadataLoader, NativeModuleLoader, ScriptLoader,
            StructDefinitionLoader,
        },
    },
    CodeStorage, LoadedFunction, ModuleStorage, RuntimeEnvironment, WithRuntimeEnvironment,
};
use move_binary_format::{
    access::ScriptAccess,
    errors::{PartialVMError, PartialVMResult, VMResult},
    CompiledModule,
};
use move_core_types::{
    identifier::IdentStr,
    language_storage::{ModuleId, TypeTag},
    vm_status::StatusCode,
};
use move_vm_types::{
    code::Code,
    gas::GasMeter,
    loaded_data::runtime_types::{StructNameIndex, StructType, Type},
    sha3_256,
};
use std::sync::Arc;

pub struct EagerLoader<'a, T> {
    module_storage: &'a T,
}

impl<'a, T> EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    pub fn new(module_storage: &'a T) -> Self {
        Self { module_storage }
    }

    fn unmetered_load_type(&self, tag: &TypeTag) -> PartialVMResult<Type> {
        self.runtime_environment()
            .vm_config()
            .ty_builder
            .create_ty(tag, |st| {
                self.module_storage
                    .unmetered_get_existing_eagerly_verified_module(&st.address, &st.module)
                    .and_then(|module| module.get_struct(&st.name))
            })
            .map_err(|err| err.to_partial())
    }

    fn unmetered_get_function_definition(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
    ) -> VMResult<(Arc<Module>, Arc<Function>)> {
        self.module_storage
            .unmetered_get_existing_eagerly_verified_module(module_id.address(), module_id.name())
            .and_then(|module| {
                let function = module.get_function(function_name)?;
                Ok((module, function))
            })
    }
}

impl<'a, T> EagerLoader<'a, T>
where
    T: CodeStorage,
{
    fn unmetered_verify_and_cache_script(&self, serialized_script: &[u8]) -> VMResult<Arc<Script>> {
        use Code::*;

        let hash = sha3_256(serialized_script);
        let deserialized_script = match self.module_storage.get_script(&hash) {
            Some(Verified(script)) => return Ok(script),
            Some(Deserialized(deserialized_script)) => deserialized_script,
            None => self
                .runtime_environment()
                .deserialize_into_script(serialized_script)
                .map(Arc::new)?,
        };

        let locally_verified_script = self
            .runtime_environment()
            .build_locally_verified_script(deserialized_script)?;

        let immediate_dependencies = locally_verified_script
            .immediate_dependencies_iter()
            .map(|(addr, name)| {
                self.module_storage
                    .unmetered_get_existing_eagerly_verified_module(addr, name)
            })
            .collect::<VMResult<Vec<_>>>()?;

        let verified_script = self.runtime_environment().build_verified_script(
            locally_verified_script,
            &immediate_dependencies,
            &hash,
        )?;

        Ok(self
            .module_storage
            .insert_verified_script(hash, verified_script))
    }
}

impl<'a, T> WithRuntimeEnvironment for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        self.module_storage.runtime_environment()
    }
}

impl<'a, T> StructDefinitionLoader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn is_lazy_loading_enabled(&self) -> bool {
        false
    }

    fn load_struct_definition(
        &self,
        _gas_meter: &mut impl GasMeter,
        _traversal_context: &mut TraversalContext,
        idx: &StructNameIndex,
    ) -> PartialVMResult<Arc<StructType>> {
        let struct_name = self
            .runtime_environment()
            .struct_name_index_map()
            .idx_to_struct_name_ref(*idx)?;

        self.module_storage
            .unmetered_get_existing_eagerly_verified_module(
                struct_name.module.address(),
                struct_name.module.name(),
            )
            .and_then(|module| module.get_struct(struct_name.name.as_ident_str()))
            .map_err(|err| err.to_partial())
    }
}

impl<'a, T> FunctionDefinitionLoader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_function_definition(
        &self,
        _gas_meter: &mut impl GasMeter,
        _traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
    ) -> VMResult<(Arc<Module>, Arc<Function>)> {
        self.unmetered_get_function_definition(module_id, function_name)
            .map_err(|err| match err.major_status() {
                StatusCode::LINKER_ERROR
                | StatusCode::UNKNOWN_BINARY_ERROR
                | StatusCode::UNKNOWN_VALIDATION_STATUS
                | StatusCode::INVALID_SIGNATURE
                | StatusCode::UNKNOWN_VERIFICATION_ERROR
                | StatusCode::UNEXPECTED_VERIFIER_ERROR
                | StatusCode::UNEXPECTED_DESERIALIZATION_ERROR
                | StatusCode::CODE_DESERIALIZATION_ERROR => err,
                _ => PartialVMError::new(StatusCode::FUNCTION_RESOLUTION_FAILURE)
                    .with_message(format!(
                        "Module or function do not exist for {}::{}::{}",
                        module_id.address(),
                        module_id.name(),
                        function_name
                    ))
                    .finish(err.location().clone()),
            })
    }
}

impl<'a, T> NativeModuleLoader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn charge_native_result_load_module(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<()> {
        let arena_id = traversal_context
            .referenced_module_ids
            .alloc(module_id.clone());
        check_dependencies_and_charge_gas(
            self.module_storage,
            gas_meter,
            traversal_context,
            [(arena_id.address(), arena_id.name())],
        )
        .map_err(|err| {
            err.to_partial().append_message_with_separator(
                '.',
                format!(
                    "Failed to charge transitive dependency for {}. Does this module exist?",
                    module_id
                ),
            )
        })?;
        Ok(())
    }
}

impl<'a, T> ModuleMetadataLoader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_module_for_metadata(
        &self,
        _gas_meter: &mut impl GasMeter,
        _traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<Arc<CompiledModule>> {
        self.module_storage
            .unmetered_get_existing_deserialized_module(module_id.address(), module_id.name())
            .map_err(|err| err.to_partial())
    }
}

impl<'a, T> InstantiatedFunctionLoaderHelper for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_ty_arg(
        &self,
        _gas_meter: &mut impl GasMeter,
        _traversal_context: &mut TraversalContext,
        ty_arg: &TypeTag,
    ) -> PartialVMResult<Type> {
        self.unmetered_load_type(ty_arg)
    }
}

impl<'a, T> InstantiatedFunctionLoader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_instantiated_function(
        &self,
        config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        if config.charge_for_ty_tag_dependencies {
            check_type_tag_dependencies_and_charge_gas(
                self.module_storage,
                gas_meter,
                traversal_context,
                ty_args,
            )?;
        }
        if config.charge_for_dependencies {
            let module_id = traversal_context
                .referenced_module_ids
                .alloc(module_id.clone());
            check_dependencies_and_charge_gas(
                self.module_storage,
                gas_meter,
                traversal_context,
                [(module_id.address(), module_id.name())],
            )?;
        }
        let (module, function) =
            self.load_function_definition(gas_meter, traversal_context, module_id, function_name)?;
        self.build_instantiated_function(gas_meter, traversal_context, module, function, ty_args)
    }
}

impl<'a, T> ScriptLoader for EagerLoader<'a, T>
where
    T: CodeStorage,
{
    fn load_script(
        &self,
        config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        serialized_script: &[u8],
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        let script = self.unmetered_verify_and_cache_script(serialized_script)?;
        if config.charge_for_ty_tag_dependencies {
            check_type_tag_dependencies_and_charge_gas(
                self.module_storage,
                gas_meter,
                traversal_context,
                ty_args,
            )?;
        }
        if config.charge_for_dependencies {
            let ids = script
                .immediate_dependencies_iter()
                .map(|(addr, name)| {
                    traversal_context
                        .referenced_module_ids
                        .alloc(ModuleId::new(*addr, name.to_owned()))
                })
                .collect::<Vec<_>>();
            check_dependencies_and_charge_gas(
                self.module_storage,
                gas_meter,
                traversal_context,
                ids.into_iter()
                    .map(|module_id| (module_id.address(), module_id.name())),
            )?;
        }
        self.build_instantiated_script(gas_meter, traversal_context, script, ty_args)
    }
}

impl<'a, T> Loader for EagerLoader<'a, T>
where
    T: ModuleStorage,
{
    fn unmetered_module_storage(&self) -> &dyn ModuleStorage {
        self.module_storage
    }
}
