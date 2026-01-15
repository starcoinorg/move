// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code)]

use crate::{
    data_cache::TransactionDataCache,
    loader::{LoadedFunction, Loader, ModuleMetadataLoader as LoaderMetadataLoader, ModuleStorageAdapter},
    module_traversal::TraversalContext,
};
use move_binary_format::errors::{PartialVMResult, VMResult};
use move_core_types::{
    identifier::IdentStr,
    language_storage::{ModuleId, TypeTag},
    metadata::Metadata,
};
use move_vm_types::{
    gas::GasMeter,
    loaded_data::runtime_types::{StructNameIndex, StructType},
};
use std::sync::Arc;

pub(crate) trait StructDefinitionLoader {
    fn load_struct_definition(
        &self,
        struct_idx: StructNameIndex,
        module_store: &ModuleStorageAdapter,
    ) -> PartialVMResult<Arc<StructType>>;
}

pub(crate) trait FunctionDefinitionLoader {
    fn load_function_definition(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction>;
}

pub(crate) trait ModuleMetadataLoader {
    fn load_module_metadata<'a, M: GasMeter>(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut M,
        traversal_context: &mut TraversalContext<'a>,
    ) -> VMResult<Vec<Metadata>>;
}

pub(crate) trait NativeModuleLoader {
    fn resolve_native_function(
        &self,
        _module_id: &ModuleId,
        _function_name: &IdentStr,
    ) -> bool;
}

pub(crate) trait ScriptLoader {
    fn load_script(
        &self,
        script: &[u8],
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction>;
}

pub(crate) trait InstantiatedFunctionLoader {
    fn load_instantiated_function(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction>;
}

pub(crate) struct EagerLoader<'a> {
    loader: &'a Loader,
}

impl<'a> EagerLoader<'a> {
    pub(crate) fn new(loader: &'a Loader) -> Self {
        Self { loader }
    }
}

impl<'a> StructDefinitionLoader for EagerLoader<'a> {
    fn load_struct_definition(
        &self,
        struct_idx: StructNameIndex,
        module_store: &ModuleStorageAdapter,
    ) -> PartialVMResult<Arc<StructType>> {
        let name = self.loader.name_cache.idx_to_identifier(struct_idx);
        module_store.get_struct_type_by_identifier(&name.name, &name.module)
    }
}

impl<'a> FunctionDefinitionLoader for EagerLoader<'a> {
    fn load_function_definition(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_function(module_id, function_name, ty_args, data_store, module_store)
    }
}

impl<'a> ModuleMetadataLoader for EagerLoader<'a> {
    fn load_module_metadata<'b, M: GasMeter>(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut M,
        traversal_context: &mut TraversalContext<'b>,
    ) -> VMResult<Vec<Metadata>> {
        LoaderMetadataLoader::get_module_metadata(
            self.loader,
            module_id,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
        )
    }
}

impl<'a> NativeModuleLoader for EagerLoader<'a> {
    fn resolve_native_function(
        &self,
        _module_id: &ModuleId,
        _function_name: &IdentStr,
    ) -> bool {
        false
    }
}

impl<'a> ScriptLoader for EagerLoader<'a> {
    fn load_script(
        &self,
        script: &[u8],
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_script(script, ty_args, data_store, module_store)
    }
}

impl<'a> InstantiatedFunctionLoader for EagerLoader<'a> {
    fn load_instantiated_function(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_function(module_id, function_name, ty_args, data_store, module_store)
    }
}

pub(crate) struct LazyLoader<'a> {
    loader: &'a Loader,
}

impl<'a> LazyLoader<'a> {
    pub(crate) fn new(loader: &'a Loader) -> Self {
        Self { loader }
    }
}

impl<'a> StructDefinitionLoader for LazyLoader<'a> {
    fn load_struct_definition(
        &self,
        struct_idx: StructNameIndex,
        module_store: &ModuleStorageAdapter,
    ) -> PartialVMResult<Arc<StructType>> {
        let name = self.loader.name_cache.idx_to_identifier(struct_idx);
        module_store.get_struct_type_by_identifier(&name.name, &name.module)
    }
}

impl<'a> FunctionDefinitionLoader for LazyLoader<'a> {
    fn load_function_definition(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_function(module_id, function_name, ty_args, data_store, module_store)
    }
}

impl<'a> ModuleMetadataLoader for LazyLoader<'a> {
    fn load_module_metadata<'b, M: GasMeter>(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut M,
        traversal_context: &mut TraversalContext<'b>,
    ) -> VMResult<Vec<Metadata>> {
        LoaderMetadataLoader::get_module_metadata(
            self.loader,
            module_id,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
        )
    }
}

impl<'a> NativeModuleLoader for LazyLoader<'a> {
    fn resolve_native_function(
        &self,
        _module_id: &ModuleId,
        _function_name: &IdentStr,
    ) -> bool {
        false
    }
}

impl<'a> ScriptLoader for LazyLoader<'a> {
    fn load_script(
        &self,
        script: &[u8],
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_script(script, ty_args, data_store, module_store)
    }
}

impl<'a> InstantiatedFunctionLoader for LazyLoader<'a> {
    fn load_instantiated_function(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        self.loader
            .load_function(module_id, function_name, ty_args, data_store, module_store)
    }
}

pub(crate) enum LoaderV2<'a> {
    Eager(EagerLoader<'a>),
    Lazy(LazyLoader<'a>),
}

impl<'a> LoaderV2<'a> {
    pub(crate) fn new(loader: &'a Loader) -> Self {
        if loader.vm_config().loader_config.enable_lazy_loading {
            Self::Lazy(LazyLoader::new(loader))
        } else {
            Self::Eager(EagerLoader::new(loader))
        }
    }
}

macro_rules! dispatch_loader {
    ($self:expr, $method:ident $(, $arg:expr)* $(,)?) => {
        match $self {
            LoaderV2::Eager(loader) => loader.$method($($arg),*),
            LoaderV2::Lazy(loader) => loader.$method($($arg),*),
        }
    };
}

impl<'a> StructDefinitionLoader for LoaderV2<'a> {
    fn load_struct_definition(
        &self,
        struct_idx: StructNameIndex,
        module_store: &ModuleStorageAdapter,
    ) -> PartialVMResult<Arc<StructType>> {
        dispatch_loader!(self, load_struct_definition, struct_idx, module_store)
    }
}

impl<'a> FunctionDefinitionLoader for LoaderV2<'a> {
    fn load_function_definition(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        dispatch_loader!(
            self,
            load_function_definition,
            module_id,
            function_name,
            ty_args,
            data_store,
            module_store,
        )
    }
}

impl<'a> ModuleMetadataLoader for LoaderV2<'a> {
    fn load_module_metadata<'b, M: GasMeter>(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut M,
        traversal_context: &mut TraversalContext<'b>,
    ) -> VMResult<Vec<Metadata>> {
        dispatch_loader!(
            self,
            load_module_metadata,
            module_id,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
        )
    }
}

impl<'a> NativeModuleLoader for LoaderV2<'a> {
    fn resolve_native_function(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
    ) -> bool {
        dispatch_loader!(self, resolve_native_function, module_id, function_name)
    }
}

impl<'a> ScriptLoader for LoaderV2<'a> {
    fn load_script(
        &self,
        script: &[u8],
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        dispatch_loader!(self, load_script, script, ty_args, data_store, module_store)
    }
}

impl<'a> InstantiatedFunctionLoader for LoaderV2<'a> {
    fn load_instantiated_function(
        &self,
        module_id: &ModuleId,
        function_name: &IdentStr,
        ty_args: &[TypeTag],
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
    ) -> VMResult<LoadedFunction> {
        dispatch_loader!(
            self,
            load_instantiated_function,
            module_id,
            function_name,
            ty_args,
            data_store,
            module_store,
        )
    }
}
