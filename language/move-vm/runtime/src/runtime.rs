// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    AsUnsyncModuleStorage,
    config::VMConfig,
    data_cache::TransactionDataCache,
    dispatch_loader,
    interpreter::Interpreter,
    loader::{LoadedFunction, Loader, ModuleCache, ModuleStorage, ModuleStorageAdapter},
    module_traversal::TraversalContext,
    native_extensions::NativeContextExtensions,
    native_functions::{NativeFunction, NativeFunctions},
    session::SerializedReturnValues,
    RuntimeEnvironment, RuntimeEnvironmentRef, StagingModuleStorage,
    WithRuntimeEnvironment,
};
use bytes::Bytes;
use move_binary_format::{
    compatibility::Compatibility,
    errors::{Location, PartialVMError, PartialVMResult, VMResult},
    file_format::LocalIndex,
    CompiledModule,
};
use move_core_types::{
    account_address::AccountAddress, identifier::Identifier, language_storage::TypeTag,
    value::MoveTypeLayout, vm_status::StatusCode,
};
use move_vm_types::{
    code::ModuleBytesStorage,
    gas::GasMeter,
    loaded_data::runtime_types::Type,
    values::{Locals, Reference, VMValueCast, Value},
};
use std::{borrow::Borrow, sync::Arc};

use crate::storage::ty_layout_converter::LayoutConverter;

/// An instantiation of the MoveVM.
pub(crate) struct VMRuntime {
    pub(crate) loader: Loader,
    pub(crate) module_cache: Arc<ModuleCache>,
    pub(crate) runtime_environment: RuntimeEnvironment,
}

struct WithEnvironment<'a, T> {
    runtime_environment: &'a RuntimeEnvironment,
    storage: &'a T,
}

impl<T> WithEnvironment<'_, T> {
    fn new<'a>(
        runtime_environment: &'a RuntimeEnvironment,
        storage: &'a T,
    ) -> WithEnvironment<'a, T> {
        WithEnvironment {
            runtime_environment,
            storage,
        }
    }
}

impl<T> WithRuntimeEnvironment for WithEnvironment<'_, T> {
    fn runtime_environment(&self) -> &RuntimeEnvironment {
        self.runtime_environment
    }
}

impl<T: ModuleBytesStorage> ModuleBytesStorage for WithEnvironment<'_, T> {
    fn fetch_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &move_core_types::identifier::IdentStr,
    ) -> VMResult<Option<Bytes>> {
        self.storage.fetch_module_bytes(address, module_name)
    }
}

impl Clone for VMRuntime {
    fn clone(&self) -> Self {
        Self {
            loader: self.loader.clone(),
            module_cache: Arc::new(ModuleCache::clone(&self.module_cache)),
            runtime_environment: self.runtime_environment.clone(),
        }
    }
}

impl VMRuntime {
    pub(crate) fn new(
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
        vm_config: VMConfig,
    ) -> PartialVMResult<Self> {
        let native_table: Vec<_> = natives.into_iter().collect();
        let native_functions = NativeFunctions::new(native_table.clone())?;
        let runtime_environment =
            RuntimeEnvironment::new_with_config(native_table, vm_config.clone());
        Ok(VMRuntime {
            loader: Loader::new_with_name_cache(
                native_functions.clone(),
                vm_config.clone(),
                runtime_environment.struct_name_index_map_arc(),
            ),
            module_cache: Arc::new(ModuleCache::new()),
            runtime_environment,
        })
    }

    pub(crate) fn runtime_environment(&self) -> &RuntimeEnvironment {
        &self.runtime_environment
    }

    pub(crate) fn publish_module_bundle(
        &self,
        modules: Vec<Vec<u8>>,
        sender: AccountAddress,
        data_store: &mut TransactionDataCache,
        _module_store: &ModuleStorageAdapter,
        _gas_meter: &mut impl GasMeter,
        compat: Compatibility,
    ) -> VMResult<()> {
        let verified_bundle = {
            let base_storage = WithEnvironment::new(self.runtime_environment(), &*data_store);
            let existing_module_storage = base_storage.as_unsync_module_storage();
            StagingModuleStorage::create_with_compat_config(
                &sender,
                compat,
                &existing_module_storage,
                modules.iter().cloned().map(Bytes::from).collect(),
            )?
            .release_verified_module_bundle()
        };

        for (module_id, blob) in verified_bundle {
            let is_republishing = data_store.exists_module(&module_id)?;
            if is_republishing {
                self.loader.mark_as_invalid();
            }
            data_store.publish_module(&module_id, blob.to_vec(), is_republishing)?;
        }
        Ok(())
    }

    pub(crate) fn verify_module_bundle_for_publication(
        &self,
        modules: &[CompiledModule],
        sender: AccountAddress,
        compat: Compatibility,
        data_store: &TransactionDataCache,
    ) -> VMResult<()> {
        let base_storage = WithEnvironment::new(self.runtime_environment(), data_store);
        let existing_module_storage = base_storage.as_unsync_module_storage();
        StagingModuleStorage::create_with_compat_config(
            &sender,
            compat,
            &existing_module_storage,
            modules
                .iter()
                .map(|module| {
                    let mut bytes = vec![];
                    module
                        .serialize(&mut bytes)
                        .map(|()| Bytes::from(bytes))
                        .map_err(|err| {
                            PartialVMError::new(
                                StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR,
                            )
                            .with_message(format!(
                                "failed to serialize verified module bundle entry: {err}"
                            ))
                            .finish(Location::Undefined)
                        })
                })
                .collect::<VMResult<Vec<_>>>()?,
        )?;
        Ok(())
    }

    pub(crate) fn deserialize_arg(
        &self,
        data_store: &TransactionDataCache,
        ty: &Type,
        arg: impl Borrow<[u8]>,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> PartialVMResult<Value> {
        let base_storage = RuntimeEnvironmentRef::new(self.runtime_environment(), data_store);
        let module_storage = base_storage.as_unsync_module_storage();
        let (layout, has_identifier_mappings) = dispatch_loader!(&module_storage, loader, {
            LayoutConverter::new(&loader)
                .type_to_type_layout_with_identifier_mappings(gas_meter, traversal_context, ty)
        })
        .map_err(|_err| {
            PartialVMError::new(StatusCode::INVALID_PARAM_TYPE_FOR_DESERIALIZATION)
                .with_message("[VM] failed to get layout from type".to_string())
        })?;

        let deserialization_error = || -> PartialVMError {
            PartialVMError::new(StatusCode::FAILED_TO_DESERIALIZE_ARGUMENT)
                .with_message("[VM] failed to deserialize argument".to_string())
        };

        // Make sure we do not construct values which might have identifiers
        // inside. This should be guaranteed by transaction argument validation
        // but because it does not use layouts we double-check here.
        if has_identifier_mappings {
            return Err(deserialization_error());
        }

        match Value::simple_deserialize(arg.borrow(), &layout) {
            Some(val) => Ok(val),
            None => Err(deserialization_error()),
        }
    }

    pub(crate) fn deserialize_args(
        &self,
        data_store: &TransactionDataCache,
        param_tys: Vec<Type>,
        serialized_args: Vec<impl Borrow<[u8]>>,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> PartialVMResult<(Locals, Vec<Value>)> {
        if param_tys.len() != serialized_args.len() {
            return Err(
                PartialVMError::new(StatusCode::NUMBER_OF_ARGUMENTS_MISMATCH).with_message(
                    format!(
                        "argument length mismatch: expected {} got {}",
                        param_tys.len(),
                        serialized_args.len()
                    ),
                ),
            );
        }

        // Create a list of dummy locals. Each value stored will be used be borrowed and passed
        // by reference to the invoked function
        let mut dummy_locals = Locals::new(param_tys.len());
        // Arguments for the invoked function. These can be owned values or references
        let deserialized_args = param_tys
            .into_iter()
            .zip(serialized_args)
            .enumerate()
            .map(|(idx, (ty, arg_bytes))| match &ty {
                Type::MutableReference(inner_t) | Type::Reference(inner_t) => {
                    dummy_locals.store_loc(
                        idx,
                        self.deserialize_arg(
                            data_store,
                            inner_t,
                            arg_bytes,
                            gas_meter,
                            traversal_context,
                        )?,
                        self.loader.vm_config().check_invariant_in_swap_loc,
                    )?;
                    dummy_locals.borrow_loc(idx)
                }
                _ => self.deserialize_arg(
                    data_store,
                    &ty,
                    arg_bytes,
                    gas_meter,
                    traversal_context,
                ),
            })
            .collect::<PartialVMResult<Vec<_>>>()?;
        Ok((dummy_locals, deserialized_args))
    }

    fn serialize_return_value(
        &self,
        data_store: &TransactionDataCache,
        ty: &Type,
        value: Value,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> PartialVMResult<(Vec<u8>, MoveTypeLayout)> {
        let (ty, value) = match ty {
            Type::Reference(inner) | Type::MutableReference(inner) => {
                let ref_value: Reference = value.cast()?;
                let inner_value = ref_value.read_ref()?;
                (&**inner, inner_value)
            }
            _ => (ty, value),
        };

        let base_storage = RuntimeEnvironmentRef::new(self.runtime_environment(), data_store);
        let module_storage = base_storage.as_unsync_module_storage();
        let (layout, has_identifier_mappings) = dispatch_loader!(&module_storage, loader, {
            LayoutConverter::new(&loader)
                .type_to_type_layout_with_identifier_mappings(gas_meter, traversal_context, ty)
        })
        .map_err(|_err| {
                // TODO: Should we use `err` instead of mapping?
                PartialVMError::new(StatusCode::VERIFICATION_ERROR).with_message(
                    "entry point functions cannot have non-serializable return types".to_string(),
                )
            })?;

        let serialization_error = || -> PartialVMError {
            PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                .with_message("failed to serialize return values".to_string())
        };

        // Disallow native values to escape through return values of a function.
        if has_identifier_mappings {
            return Err(serialization_error());
        }

        let bytes = value
            .simple_serialize(&layout)
            .ok_or_else(serialization_error)?;
        Ok((bytes, layout))
    }

    fn serialize_return_values(
        &self,
        data_store: &TransactionDataCache,
        return_types: &[Type],
        return_values: Vec<Value>,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> PartialVMResult<Vec<(Vec<u8>, MoveTypeLayout)>> {
        if return_types.len() != return_values.len() {
            return Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR).with_message(
                    format!(
                        "declared {} return types, but got {} return values",
                        return_types.len(),
                        return_values.len()
                    ),
                ),
            );
        }

        return_types
            .iter()
            .zip(return_values)
            .map(|(ty, value)| {
                self.serialize_return_value(
                    data_store,
                    ty,
                    value,
                    gas_meter,
                    traversal_context,
                )
            })
            .collect()
    }

    fn execute_function_impl(
        &self,
        func: LoadedFunction,
        serialized_args: Vec<impl Borrow<[u8]>>,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        extensions: &mut NativeContextExtensions,
    ) -> VMResult<SerializedReturnValues> {
        let LoadedFunction { ty_args, function } = func;
        let ty_builder = self.loader().ty_builder();

        let param_tys = function
            .param_tys()
            .iter()
            .map(|ty| ty_builder.create_ty_with_subst(ty, &ty_args))
            .collect::<PartialVMResult<Vec<_>>>()
            .map_err(|err| err.finish(Location::Undefined))?;
        let mut_ref_args = param_tys
            .iter()
            .enumerate()
            .filter_map(|(idx, ty)| match ty {
                Type::MutableReference(inner) => Some((idx, inner.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (mut dummy_locals, deserialized_args) = self
            .deserialize_args(
                data_store,
                param_tys,
                serialized_args,
                gas_meter,
                traversal_context,
            )
            .map_err(|e| e.finish(Location::Undefined))?;
        let return_tys = function
            .return_tys()
            .iter()
            .map(|ty| ty_builder.create_ty_with_subst(ty, &ty_args))
            .collect::<PartialVMResult<Vec<_>>>()
            .map_err(|err| err.finish(Location::Undefined))?;

        let return_values = Interpreter::entrypoint(
            function,
            ty_args,
            deserialized_args,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
            extensions,
            &self.loader,
        )?;

        let serialized_return_values = self
            .serialize_return_values(
                data_store,
                &return_tys,
                return_values,
                gas_meter,
                traversal_context,
            )
            .map_err(|e| e.finish(Location::Undefined))?;
        let serialized_mut_ref_outputs = mut_ref_args
            .into_iter()
            .map(|(idx, ty)| {
                // serialize return values first in the case that a value points into this local
                let local_val = dummy_locals
                    .move_loc(idx, self.loader.vm_config().check_invariant_in_swap_loc)?;
                let (bytes, layout) = self.serialize_return_value(
                    data_store,
                    &ty,
                    local_val,
                    gas_meter,
                    traversal_context,
                )?;
                Ok((idx as LocalIndex, bytes, layout))
            })
            .collect::<PartialVMResult<_>>()
            .map_err(|e| e.finish(Location::Undefined))?;

        // locals should not be dropped until all return values are serialized
        drop(dummy_locals);

        Ok(SerializedReturnValues {
            mutable_reference_outputs: serialized_mut_ref_outputs,
            return_values: serialized_return_values,
        })
    }

    pub(crate) fn execute_function_instantiation(
        &self,
        func: LoadedFunction,
        serialized_args: Vec<impl Borrow<[u8]>>,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        extensions: &mut NativeContextExtensions,
    ) -> VMResult<SerializedReturnValues> {
        self.execute_function_impl(
            func,
            serialized_args,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
            extensions,
        )
    }

    pub(crate) fn execute_script(
        &self,
        script: impl Borrow<[u8]>,
        ty_args: Vec<TypeTag>,
        serialized_args: Vec<impl Borrow<[u8]>>,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        extensions: &mut NativeContextExtensions,
    ) -> VMResult<()> {
        // Load the script first, verify it, and then execute the entry-point main function.
        let main = self
            .loader
            .load_script_v2(
                script.borrow(),
                &ty_args,
                data_store,
                module_store,
                gas_meter,
                traversal_context,
            )?;
        self.execute_function_impl(
            main,
            serialized_args,
            data_store,
            module_store,
            gas_meter,
            traversal_context,
            extensions,
        )?;
        Ok(())
    }

    pub(crate) fn loader(&self) -> &Loader {
        &self.loader
    }

    pub(crate) fn module_storage(&self) -> Arc<dyn ModuleStorage> {
        self.module_cache.clone() as Arc<dyn ModuleStorage>
    }

    pub(crate) fn update_native_functions(
        &mut self,
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
    ) -> PartialVMResult<()> {
        let native_table: Vec<_> = natives.into_iter().collect();
        let native_functions = NativeFunctions::new(native_table.clone())?;
        self.loader.update_native_functions(native_table)?;
        self.runtime_environment.set_natives(native_functions);
        Ok(())
    }
}
