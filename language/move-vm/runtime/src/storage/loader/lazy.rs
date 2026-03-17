// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
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
    errors::{Location, PartialVMError, PartialVMResult, VMResult},
    CompiledModule,
};
use move_core_types::{
    gas_algebra::NumBytes,
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

pub struct LazyLoader<'a, T> {
    module_storage: &'a T,
}

impl<'a, T> LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    pub fn new(module_storage: &'a T) -> Self {
        Self { module_storage }
    }

    fn charge_module(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<()> {
        if traversal_context.visit_if_not_special_module_id(module_id) {
            let addr = module_id.address();
            let name = module_id.name();
            let size = self
                .module_storage
                .unmetered_get_existing_module_size(addr, name)
                .map_err(|err| err.to_partial())?;
            gas_meter.charge_dependency(false, addr, name, NumBytes::new(size as u64))?;
        }
        Ok(())
    }

    fn metered_load_module(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> VMResult<Arc<Module>> {
        self.charge_module(gas_meter, traversal_context, module_id)
            .map_err(|err| err.finish(Location::Undefined))?;
        self.module_storage
            .unmetered_get_existing_lazily_verified_module(module_id)
    }

    fn metered_load_type(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        tag: &TypeTag,
    ) -> PartialVMResult<Type> {
        self.runtime_environment()
            .vm_config()
            .ty_builder
            .create_ty(tag, |st| {
                self.metered_load_module(
                    gas_meter,
                    traversal_context,
                    &ModuleId::new(st.address, st.module.to_owned()),
                )
                .and_then(|module| module.get_struct(&st.name))
            })
            .map_err(|err| err.to_partial())
    }
}

impl<'a, T> LazyLoader<'a, T>
where
    T: CodeStorage,
{
    fn metered_verify_and_cache_script(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        serialized_script: &[u8],
    ) -> VMResult<Arc<Script>> {
        use Code::*;

        let hash = sha3_256(serialized_script);
        let deserialized_script = match self.module_storage.get_script(&hash) {
            Some(Verified(script)) => {
                for (addr, name) in script.immediate_dependencies_iter() {
                    let module_id = ModuleId::new(*addr, name.to_owned());
                    self.charge_module(gas_meter, traversal_context, &module_id)
                        .map_err(|err| err.finish(Location::Undefined))?;
                }
                return Ok(script);
            }
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
                let module_id = ModuleId::new(*addr, name.to_owned());
                self.metered_load_module(gas_meter, traversal_context, &module_id)
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

impl<'a, T> WithRuntimeEnvironment for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        self.module_storage.runtime_environment()
    }
}

impl<'a, T> StructDefinitionLoader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn is_lazy_loading_enabled(&self) -> bool {
        true
    }

    fn load_struct_definition(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        idx: &StructNameIndex,
    ) -> PartialVMResult<Arc<StructType>> {
        let struct_name = self
            .runtime_environment()
            .struct_name_index_map()
            .idx_to_struct_name_ref(*idx)?;

        self.metered_load_module(gas_meter, traversal_context, &struct_name.module)
            .and_then(|module| module.get_struct(struct_name.name.as_ident_str()))
            .map_err(|err| err.to_partial())
    }
}

impl<'a, T> FunctionDefinitionLoader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_function_definition(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
    ) -> VMResult<(Arc<Module>, Arc<Function>)> {
        self.metered_load_module(gas_meter, traversal_context, module_id)
            .and_then(|module| {
                let function = module.get_function(function_name)?;
                Ok((module, function))
            })
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

impl<'a, T> NativeModuleLoader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn charge_native_result_load_module(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<()> {
        self.charge_module(gas_meter, traversal_context, module_id)
    }
}

impl<'a, T> ModuleMetadataLoader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_module_for_metadata(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<Arc<CompiledModule>> {
        self.charge_module(gas_meter, traversal_context, module_id)?;
        self.module_storage
            .unmetered_get_existing_deserialized_module(module_id.address(), module_id.name())
            .map_err(|err| err.to_partial())
    }
}

impl<'a, T> InstantiatedFunctionLoaderHelper for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_ty_arg(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty_arg: &TypeTag,
    ) -> PartialVMResult<Type> {
        self.metered_load_type(gas_meter, traversal_context, ty_arg)
    }
}

impl<'a, T> InstantiatedFunctionLoader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn load_instantiated_function(
        &self,
        _config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        check_type_tag_dependencies_and_charge_gas(
            self.module_storage,
            gas_meter,
            traversal_context,
            ty_args,
        )?;
        let (module, function) =
            self.load_function_definition(gas_meter, traversal_context, module_id, function_name)?;
        self.build_instantiated_function(gas_meter, traversal_context, module, function, ty_args)
    }
}

impl<'a, T> ScriptLoader for LazyLoader<'a, T>
where
    T: CodeStorage,
{
    fn load_script(
        &self,
        _config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        serialized_script: &[u8],
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        check_type_tag_dependencies_and_charge_gas(
            self.module_storage,
            gas_meter,
            traversal_context,
            ty_args,
        )?;
        let script =
            self.metered_verify_and_cache_script(gas_meter, traversal_context, serialized_script)?;
        self.build_instantiated_script(gas_meter, traversal_context, script, ty_args)
    }
}

impl<'a, T> Loader for LazyLoader<'a, T>
where
    T: ModuleStorage,
{
    fn unmetered_module_storage(&self) -> &dyn ModuleStorage {
        self.module_storage
    }
}
