// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    loader::{Function, LoadedFunction, Module, Script},
    module_traversal::TraversalContext,
    ModuleStorage, WithRuntimeEnvironment,
};
use move_binary_format::{
    errors::{Location, PartialVMError, PartialVMResult, VMResult},
    CompiledModule,
};
use move_core_types::{
    identifier::IdentStr,
    language_storage::{ModuleId, TypeTag},
    vm_status::{sub_status::type_resolution_failure::EUSER_TYPE_LOADING_FAILURE, StatusCode},
};
use move_vm_types::{
    gas::GasMeter,
    loaded_data::{
        runtime_types::{legacy_count_type_nodes, StructNameIndex, StructType, Type, TypeBuilder},
    },
};
use std::sync::Arc;

pub trait StructDefinitionLoader: WithRuntimeEnvironment {
    fn is_lazy_loading_enabled(&self) -> bool;

    fn load_struct_definition(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        idx: &StructNameIndex,
    ) -> PartialVMResult<Arc<StructType>>;
}

pub trait FunctionDefinitionLoader {
    fn load_function_definition(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
    ) -> VMResult<(Arc<Module>, Arc<Function>)>;
}

pub trait NativeModuleLoader {
    fn charge_native_result_load_module(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<()>;
}

pub trait ModuleMetadataLoader {
    fn load_module_for_metadata(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
    ) -> PartialVMResult<Arc<CompiledModule>>;
}

pub struct LegacyLoaderConfig {
    pub charge_for_dependencies: bool,
    pub charge_for_ty_tag_dependencies: bool,
}

impl LegacyLoaderConfig {
    pub fn unmetered() -> Self {
        Self {
            charge_for_dependencies: false,
            charge_for_ty_tag_dependencies: false,
        }
    }
}

pub(crate) trait InstantiatedFunctionLoaderHelper: WithRuntimeEnvironment {
    fn load_ty_arg(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty_arg: &TypeTag,
    ) -> PartialVMResult<Type>;

    fn build_instantiated_function(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module: Arc<Module>,
        function: Arc<Function>,
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        let ty_args = ty_args
            .iter()
            .map(|ty_arg| {
                self.load_ty_arg(gas_meter, traversal_context, ty_arg)
                    .map_err(|err| err.finish(Location::Undefined))
            })
            .collect::<VMResult<Vec<_>>>()
            .map_err(|mut err| {
                if StatusCode::TYPE_RESOLUTION_FAILURE == err.major_status() {
                    err.set_sub_status(EUSER_TYPE_LOADING_FAILURE);
                }
                err
            })?;

        Type::verify_ty_arg_abilities(function.ty_param_abilities(), &ty_args)
            .map_err(|e| e.finish(Location::Module(module.self_id().clone())))?;

        Ok(LoadedFunction { ty_args, function })
    }

    fn build_instantiated_script(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        script: Arc<Script>,
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction> {
        let ty_args = ty_args
            .iter()
            .map(|ty_tag| self.load_ty_arg(gas_meter, traversal_context, ty_tag))
            .collect::<PartialVMResult<Vec<_>>>()
            .map_err(|err| err.finish(Location::Script))?;

        let ty_builder = &self.runtime_environment().vm_config().ty_builder;
        if ty_builder.is_legacy()
            && ty_args.iter().map(legacy_count_type_nodes).sum::<u64>()
                > TypeBuilder::LEGACY_MAX_TYPE_INSTANTIATION_NODES
        {
            return Err(PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES).finish(Location::Script));
        }

        let main = script.entry_point();
        Type::verify_ty_arg_abilities(main.ty_param_abilities(), &ty_args)
            .map_err(|err| err.finish(Location::Script))?;

        Ok(LoadedFunction {
            ty_args,
            function: main,
        })
    }
}

#[allow(private_bounds)]
pub trait InstantiatedFunctionLoader: InstantiatedFunctionLoaderHelper {
    fn load_instantiated_function(
        &self,
        config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction>;
}

pub trait Loader:
    FunctionDefinitionLoader
    + ModuleMetadataLoader
    + NativeModuleLoader
    + StructDefinitionLoader
    + InstantiatedFunctionLoader
{
    fn unmetered_module_storage(&self) -> &dyn ModuleStorage;
}

#[allow(private_bounds)]
pub trait ScriptLoader: InstantiatedFunctionLoaderHelper {
    fn load_script(
        &self,
        config: &LegacyLoaderConfig,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        serialized_script: &[u8],
        ty_args: &[TypeTag],
    ) -> VMResult<LoadedFunction>;
}
