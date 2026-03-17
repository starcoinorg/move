// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::VMConfig,
    data_cache::TransactionDataCache,
    dispatch_loader,
    logging::expect_no_verification_errors_unless_bogus_storage,
    module_traversal::TraversalContext,
    native_functions::NativeFunctions,
    AsUnsyncModuleStorage, ModuleStorage as LoaderV2ModuleStorage, NativeModuleLoader,
    StructDefinitionLoader, WithRuntimeEnvironment,
};
use hashbrown::Equivalent;
use lazy_static::lazy_static;
use move_binary_format::{
    access::{ModuleAccess, ScriptAccess},
    errors::{Location, PartialVMError, PartialVMResult, VMResult},
    file_format::{
        Constant, ConstantPoolIndex, FieldHandleIndex, FieldInstantiationIndex,
        FunctionHandleIndex, FunctionInstantiationIndex, SignatureIndex,
        StructDefInstantiationIndex, StructDefinitionIndex,
    },
};
use move_core_types::{
    account_address::AccountAddress,
    gas_algebra::NumTypeNodes,
    identifier::IdentStr,
    language_storage::ModuleId,
    vm_status::StatusCode,
};
use move_vm_types::{
    code::ModuleBytesStorage,
    gas::GasMeter,
    loaded_data::{
        runtime_types::{AbilityInfo, StructNameIndex, StructType, Type},
        struct_name_indexing::StructNameIndexMap,
    },
};
use parking_lot::{Mutex, RwLock};
use std::{
    collections::BTreeSet,
    hash::Hash,
    sync::Arc,
};

mod access_specifier_loader;
mod function;
mod modules;
mod script;
mod type_loader;

use crate::native_functions::NativeFunction;
pub use function::LoadedFunction;
pub(crate) use function::{Function, FunctionHandle, FunctionInstantiation, Scope};
pub(crate) use modules::{Module, ModuleCache, ModuleStorage, ModuleStorageAdapter};
use move_core_types::identifier::Identifier;
use move_vm_types::loaded_data::runtime_types::{legacy_count_type_nodes, TypeBuilder};
pub(crate) use script::{Script, ScriptCache};
use type_loader::intern_type;

type ScriptHash = [u8; 32];

struct LoaderV2DataStore<'a, T> {
    runtime_environment: crate::RuntimeEnvironment,
    storage: &'a T,
}

impl<'a, T> LoaderV2DataStore<'a, T> {
    fn new(runtime_environment: crate::RuntimeEnvironment, storage: &'a T) -> Self {
        Self {
            runtime_environment,
            storage,
        }
    }
}

impl<T: ModuleBytesStorage> ModuleBytesStorage for LoaderV2DataStore<'_, T> {
    fn fetch_module_bytes(
        &self,
        address: &AccountAddress,
        module_name: &IdentStr,
    ) -> VMResult<Option<bytes::Bytes>> {
        self.storage.fetch_module_bytes(address, module_name)
    }
}

impl<T> crate::WithRuntimeEnvironment for LoaderV2DataStore<'_, T> {
    fn runtime_environment(&self) -> &crate::RuntimeEnvironment {
        &self.runtime_environment
    }
}

// A simple cache that offers both a HashMap and a Vector lookup.
// Values are forced into a `Arc` so they can be used from multiple thread.
// Access to this cache is always under a `RwLock`.
#[derive(Clone)]
pub(crate) struct BinaryCache<K, V> {
    // Notice that we are using the HashMap implementation from the hashbrown crate, not the
    // one from std, as it allows alternative key representations to be used for lookup,
    // making certain optimizations possible.
    id_map: hashbrown::HashMap<K, usize>,
    binaries: Vec<Arc<V>>,
}

impl<K, V> BinaryCache<K, V>
where
    K: Eq + Hash,
{
    fn new() -> Self {
        Self {
            id_map: hashbrown::HashMap::new(),
            binaries: vec![],
        }
    }

    fn insert(&mut self, key: K, binary: V) -> &Arc<V> {
        self.binaries.push(Arc::new(binary));
        let idx = self.binaries.len() - 1;
        self.id_map.insert(key, idx);
        self.binaries
            .last()
            .expect("BinaryCache: last() after push() impossible failure")
    }

    fn get<Q>(&self, key: &Q) -> Option<&Arc<V>>
    where
        Q: Hash + Eq + Equivalent<K>,
    {
        let index = self.id_map.get(key)?;
        self.binaries.get(*index)
    }
}

// Max number of modules that can skip re-verification.
const VERIFIED_CACHE_SIZE: usize = 100_000;

// Cache for already verified modules
lazy_static! {
    static ref VERIFIED_MODULES: Mutex<lru::LruCache<[u8; 32], ()>> =
        Mutex::new(lru::LruCache::new(VERIFIED_CACHE_SIZE));
}

pub(crate) type StructNameCache = StructNameIndexMap;

//
// Loader
//

// A Loader is responsible to load scripts and modules and holds the cache of all loaded
// entities. Each cache is protected by a `RwLock`. Operation in the Loader must be thread safe
// (operating on values on the stack) and when cache needs updating the mutex must be taken.
// The `pub(crate)` API is what a Loader offers to the runtime.
pub(crate) struct Loader {
    scripts: RwLock<ScriptCache>,
    natives: NativeFunctions,
    pub(crate) name_cache: Arc<StructNameCache>,

    // The below field supports a hack to workaround well-known issues with the
    // loader cache. This cache is not designed to support module upgrade or deletion.
    // This leads to situations where the cache does not reflect the state of storage:
    //
    // 1. On module upgrade, the upgraded module is in storage, but the old one still in the cache.
    // 2. On an abandoned code publishing transaction, the cache may contain a module which was
    //    never committed to storage by the adapter.
    //
    // The solution is to add a flag to Loader marking it as 'invalidated'. For scenario (1),
    // the VM sets the flag itself. For scenario (2), a public API allows the adapter to set
    // the flag.
    //
    // If the cache is invalidated, it can (and must) still be used until there are no more
    // sessions alive which are derived from a VM with this loader. This is because there are
    // internal data structures derived from the loader which can become inconsistent. Therefore
    // the adapter must explicitly call a function to flush the invalidated loader.
    //
    // This code (the loader) needs a complete refactoring. The new loader should
    //   performance. This is essential for a cache like this in a multi-tenant execution
    //   environment.
    // - should delegate lifetime ownership to the adapter. Code loading (including verification)
    //   is a major execution bottleneck. We should be able to reuse a cache for the lifetime of
    //   the adapter/node, not just a VM or even session (as effectively today).
    invalidated: RwLock<bool>,

    // Collects the cache hits on module loads. This information can be read and reset by
    // an adapter to reason about read/write conflicts of code publishing transactions and
    // other transactions.
    module_cache_hits: RwLock<BTreeSet<ModuleId>>,

    vm_config: VMConfig,
}

impl Clone for Loader {
    fn clone(&self) -> Self {
        Self {
            scripts: RwLock::new(self.scripts.read().clone()),
            natives: self.natives.clone(),
            name_cache: self.name_cache.clone(),
            invalidated: RwLock::new(*self.invalidated.read()),
            module_cache_hits: RwLock::new(self.module_cache_hits.read().clone()),
            vm_config: self.vm_config.clone(),
        }
    }
}

impl Loader {
    pub(crate) fn new_with_name_cache(
        natives: NativeFunctions,
        vm_config: VMConfig,
        name_cache: Arc<StructNameCache>,
    ) -> Self {
        Self {
            scripts: RwLock::new(ScriptCache::new()),
            name_cache,
            natives,
            invalidated: RwLock::new(false),
            module_cache_hits: RwLock::new(BTreeSet::new()),
            vm_config,
        }
    }

    pub(crate) fn vm_config(&self) -> &VMConfig {
        &self.vm_config
    }

    pub(crate) fn ensure_module_loaded_v2(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        module_store: &ModuleStorageAdapter,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> VMResult<()> {
        if module_store.module_at(module_id).is_some() {
            return Ok(());
        }

        let runtime_environment = self.runtime_environment();
        let base_storage = LoaderV2DataStore::new(runtime_environment, &*data_store);
        let module_storage = base_storage.into_unsync_module_storage();
        let result = dispatch_loader!(&module_storage, loader, {
            loader
                .charge_native_result_load_module(gas_meter, traversal_context, module_id)
                .map_err(|err| err.finish(Location::Undefined))?;
            if self.vm_config.enable_lazy_loading {
                module_storage
                    .unmetered_get_existing_lazily_verified_module(module_id)
                    .map(|_| ())
            } else {
                module_storage
                    .unmetered_get_existing_eagerly_verified_module(
                        module_id.address(),
                        module_id.name(),
                    )
                    .map(|_| ())
            }
        });
        let (_ctx, verified_modules_iter) = module_storage.unpack_into_verified_modules_iter();
        let result = match result {
            Ok(()) => Ok(()),
            Err(err) => match data_store.exists_module(module_id) {
                Ok(true) => Err(expect_no_verification_errors_unless_bogus_storage(err)),
                _ => Err(err),
            },
        }?;
        for (_module_id, module) in verified_modules_iter {
            module_store.store_verified_module(module);
        }
        Ok(result)
    }

    pub(crate) fn ty_builder(&self) -> &TypeBuilder {
        &self.vm_config.ty_builder
    }

    /// Flush this cache if it is marked as invalidated.
    // equals empty()
    pub(crate) fn flush_if_invalidated(&self) {
        let mut invalidated = self.invalidated.write();
        if *invalidated {
            *self.scripts.write() = ScriptCache::new();
            *invalidated = false;
        }
    }

    /// Mark this cache as invalidated.
    pub(crate) fn mark_as_invalid(&self) {
        *self.invalidated.write() = true;
    }

    /// Check whether this cache is invalidated.
    pub(crate) fn is_invalidated(&self) -> bool {
        *self.invalidated.read()
    }

    pub(crate) fn runtime_environment(&self) -> crate::RuntimeEnvironment {
        crate::RuntimeEnvironment::new_with_shared_name_cache(
            self.natives.clone(),
            self.vm_config.clone(),
            self.name_cache.clone(),
        )
    }

    pub(crate) fn cache_verified_script(&self, hash: ScriptHash, script: Arc<Script>) {
        self.scripts
            .write()
            .scripts
            .insert(hash, script.as_ref().clone());
    }

    //
    // Module verification and loading
    //

    //
    // Internal helpers
    //

    fn get_script(&self, hash: &ScriptHash) -> Arc<Script> {
        Arc::clone(
            self.scripts
                .read()
                .scripts
                .get(hash)
                .expect("Script hash on Function must exist"),
        )
    }
}

//
// Resolver
//

// A simple wrapper for a `Module` or a `Script` in the `Resolver`
enum BinaryType {
    Module(Arc<Module>),
    Script(Arc<Script>),
}

// A Resolver is a simple and small structure allocated on the stack and used by the
// interpreter. It's the only API known to the interpreter and it's tailored to the interpreter
// needs.
pub(crate) struct Resolver<'a> {
    loader: &'a Loader,
    module_store: &'a ModuleStorageAdapter,
    binary: BinaryType,
}

struct LoadedStructDefinitionLoader<'a> {
    runtime_environment: crate::RuntimeEnvironment,
    loader: &'a Loader,
    module_store: &'a ModuleStorageAdapter,
}

impl WithRuntimeEnvironment for LoadedStructDefinitionLoader<'_> {
    fn runtime_environment(&self) -> &crate::RuntimeEnvironment {
        &self.runtime_environment
    }
}

impl StructDefinitionLoader for LoadedStructDefinitionLoader<'_> {
    fn is_lazy_loading_enabled(&self) -> bool {
        self.loader.vm_config().enable_lazy_loading
    }

    fn load_struct_definition(
        &self,
        _gas_meter: &mut impl GasMeter,
        _traversal_context: &mut TraversalContext,
        idx: &StructNameIndex,
    ) -> PartialVMResult<Arc<StructType>> {
        let struct_name = self
            .loader
            .runtime_environment()
            .struct_name_index_map()
            .idx_to_struct_name_ref(*idx)?;
        let module = self
            .module_store
            .module_at(&struct_name.module)
            .ok_or_else(|| {
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR).with_message(
                    format!(
                        "Module {} not loaded while resolving layout for {}::{}",
                        struct_name.module, struct_name.module, struct_name.name
                    ),
                )
            })?;
        module
            .get_struct(struct_name.name.as_ident_str())
            .map_err(|err| err.to_partial())
    }
}

impl<'a> Resolver<'a> {
    fn for_module(
        loader: &'a Loader,
        module_store: &'a ModuleStorageAdapter,
        module: Arc<Module>,
    ) -> Self {
        let binary = BinaryType::Module(module);
        Self {
            loader,
            binary,
            module_store,
        }
    }

    fn for_script(
        loader: &'a Loader,
        module_store: &'a ModuleStorageAdapter,
        script: Arc<Script>,
    ) -> Self {
        let binary = BinaryType::Script(script);
        Self {
            loader,
            binary,
            module_store,
        }
    }

    //
    // Constant resolution
    //

    pub(crate) fn constant_at(&self, idx: ConstantPoolIndex) -> &Constant {
        match &self.binary {
            BinaryType::Module(module) => module.module.constant_at(idx),
            BinaryType::Script(script) => script.script.constant_at(idx),
        }
    }

    //
    // Function resolution
    //

    fn maybe_charge_and_load_module(
        &self,
        module_id: &ModuleId,
        data_store: &mut TransactionDataCache,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> VMResult<()> {
        self.loader.ensure_module_loaded_v2(
            module_id,
            data_store,
            self.module_store,
            gas_meter,
            traversal_context,
        )
    }

    pub(crate) fn function_from_handle_with_context(
        &self,
        idx: FunctionHandleIndex,
        data_store: &mut TransactionDataCache,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> VMResult<Arc<Function>> {
        let handle = match &self.binary {
            BinaryType::Module(module) => module.function_at(idx.0),
            BinaryType::Script(script) => script.function_at(idx.0),
        };

        match handle {
            FunctionHandle::Local(func) => Ok(func.clone()),
            FunctionHandle::Remote { module, name } => {
                self.maybe_charge_and_load_module(
                    module,
                    data_store,
                    gas_meter,
                    traversal_context,
                )?;
                self.module_store
                    .resolve_function_by_name(name.as_ident_str(), module)
                    .map_err(|err| err.finish(Location::Undefined))
            }
        }
    }

    pub(crate) fn function_from_instantiation_with_context(
        &self,
        idx: FunctionInstantiationIndex,
        data_store: &mut TransactionDataCache,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> VMResult<Arc<Function>> {
        let handle = match &self.binary {
            BinaryType::Module(module) => &module.function_instantiation_at(idx.0).handle,
            BinaryType::Script(script) => &script.function_instantiation_at(idx.0).handle,
        };

        match handle {
            FunctionHandle::Local(func) => Ok(func.clone()),
            FunctionHandle::Remote { module, name } => {
                self.maybe_charge_and_load_module(
                    module,
                    data_store,
                    gas_meter,
                    traversal_context,
                )?;
                self.module_store
                    .resolve_function_by_name(name.as_ident_str(), module)
                    .map_err(|err| err.finish(Location::Undefined))
            }
        }
    }

    pub(crate) fn function_from_name_with_context(
        &self,
        module_id: &ModuleId,
        func_name: &IdentStr,
        data_store: &mut TransactionDataCache,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
    ) -> VMResult<Arc<Function>> {
        self.maybe_charge_and_load_module(module_id, data_store, gas_meter, traversal_context)?;
        self.module_store
            .resolve_function_by_name(func_name, module_id)
            .map_err(|err| err.finish(Location::Undefined))
    }

    pub(crate) fn instantiate_generic_function(
        &self,
        gas_meter: Option<&mut impl GasMeter>,
        idx: FunctionInstantiationIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Vec<Type>> {
        let func_inst = match &self.binary {
            BinaryType::Module(module) => module.function_instantiation_at(idx.0),
            BinaryType::Script(script) => script.function_instantiation_at(idx.0),
        };

        if let Some(gas_meter) = gas_meter {
            for ty in &func_inst.instantiation {
                gas_meter
                    .charge_create_ty(NumTypeNodes::new(ty.num_nodes_in_subst(ty_args)? as u64))?;
            }
        }

        let ty_builder = self.loader().ty_builder();
        let mut instantiation = vec![];
        for ty in &func_inst.instantiation {
            let ty = ty_builder.create_ty_with_subst_with_legacy_check(ty, ty_args)?;
            instantiation.push(ty);
        }

        if ty_builder.is_legacy() {
            // Check if the function instantiation over all generics is larger
            // than MAX_TYPE_INSTANTIATION_NODES.
            let mut sum_nodes = 1u64;
            for ty in ty_args.iter().chain(instantiation.iter()) {
                sum_nodes = sum_nodes.saturating_add(legacy_count_type_nodes(ty));
                if sum_nodes > TypeBuilder::LEGACY_MAX_TYPE_INSTANTIATION_NODES {
                    return Err(PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES));
                }
            }
        }

        Ok(instantiation)
    }

    //
    // Type resolution
    //

    pub(crate) fn get_struct_ty(&self, idx: StructDefinitionIndex) -> Type {
        let struct_ty = match &self.binary {
            BinaryType::Module(module) => module.struct_at(idx),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        };

        self.loader()
            .ty_builder()
            .create_struct_ty(struct_ty.idx, AbilityInfo::struct_(struct_ty.abilities))
    }

    pub(crate) fn get_generic_struct_ty(
        &self,
        idx: StructDefInstantiationIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Type> {
        let struct_inst = match &self.binary {
            BinaryType::Module(module) => module.struct_instantiation_at(idx.0),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        };

        let ty_builder = self.loader().ty_builder();
        if ty_builder.is_legacy() {
            let mut sum_nodes = 1u64;
            for ty in ty_args.iter().chain(struct_inst.instantiation.iter()) {
                sum_nodes = sum_nodes.saturating_add(legacy_count_type_nodes(ty));
                if sum_nodes > TypeBuilder::LEGACY_MAX_TYPE_INSTANTIATION_NODES {
                    return Err(
                        PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES).with_message(format!(
                            "Number of type instantiation nodes exceeded the maximum of {}",
                            TypeBuilder::LEGACY_MAX_TYPE_INSTANTIATION_NODES
                        )),
                    );
                }
            }
        }

        let struct_ty = &struct_inst.definition_struct_type;
        ty_builder.create_struct_instantiation_ty_with_legacy_check(
            struct_ty,
            &struct_inst.instantiation,
            ty_args,
        )
    }

    pub(crate) fn get_field_ty(&self, idx: FieldHandleIndex) -> PartialVMResult<&Type> {
        match &self.binary {
            BinaryType::Module(module) => {
                let handle = &module.field_handles[idx.0 as usize];
                Ok(&handle.definition_struct_type.field_tys[handle.offset])
            }
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        }
    }

    pub(crate) fn get_generic_field_ty(
        &self,
        idx: FieldInstantiationIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Type> {
        let field_instantiation = match &self.binary {
            BinaryType::Module(module) => &module.field_instantiations[idx.0 as usize],
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        };

        let ty_builder = self.loader().ty_builder();
        let instantiation_tys = field_instantiation
            .instantiation
            .iter()
            .map(|inst_ty| ty_builder.create_ty_with_subst(inst_ty, ty_args))
            .collect::<PartialVMResult<Vec<_>>>()?;

        let field_ty =
            &field_instantiation.definition_struct_type.field_tys[field_instantiation.offset];
        ty_builder.create_ty_with_subst(field_ty, &instantiation_tys)
    }

    pub(crate) fn get_struct_field_tys(
        &self,
        idx: StructDefinitionIndex,
    ) -> PartialVMResult<Arc<StructType>> {
        match &self.binary {
            BinaryType::Module(module) => Ok(module.struct_at(idx)),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        }
    }

    pub(crate) fn instantiate_generic_struct_fields(
        &self,
        idx: StructDefInstantiationIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Vec<Type>> {
        let struct_inst = match &self.binary {
            BinaryType::Module(module) => module.struct_instantiation_at(idx.0),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        };
        let struct_ty = &struct_inst.definition_struct_type;

        let ty_builder = self.loader().ty_builder();
        let instantiation_tys = struct_inst
            .instantiation
            .iter()
            .map(|inst_ty| ty_builder.create_ty_with_subst(inst_ty, ty_args))
            .collect::<PartialVMResult<Vec<_>>>()?;

        struct_ty
            .field_tys
            .iter()
            .map(|inst_ty| ty_builder.create_ty_with_subst(inst_ty, &instantiation_tys))
            .collect::<PartialVMResult<Vec<_>>>()
    }

    fn single_type_at(&self, idx: SignatureIndex) -> &Type {
        match &self.binary {
            BinaryType::Module(module) => module.single_type_at(idx),
            BinaryType::Script(script) => script.single_type_at(idx),
        }
    }

    pub(crate) fn instantiate_single_type(
        &self,
        idx: SignatureIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Type> {
        let ty = self.single_type_at(idx);

        if !ty_args.is_empty() {
            self.loader()
                .ty_builder()
                .create_ty_with_subst_with_legacy_check(ty, ty_args)
        } else {
            Ok(ty.clone())
        }
    }

    //
    // Fields resolution
    //

    pub(crate) fn field_offset(&self, idx: FieldHandleIndex) -> usize {
        match &self.binary {
            BinaryType::Module(module) => module.field_offset(idx),
            BinaryType::Script(_) => unreachable!("Scripts cannot have field instructions"),
        }
    }

    pub(crate) fn field_instantiation_offset(&self, idx: FieldInstantiationIndex) -> usize {
        match &self.binary {
            BinaryType::Module(module) => module.field_instantiation_offset(idx),
            BinaryType::Script(_) => unreachable!("Scripts cannot have field instructions"),
        }
    }

    pub(crate) fn field_count(&self, idx: StructDefinitionIndex) -> u16 {
        match &self.binary {
            BinaryType::Module(module) => module.field_count(idx.0),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        }
    }

    pub(crate) fn field_instantiation_count(&self, idx: StructDefInstantiationIndex) -> u16 {
        match &self.binary {
            BinaryType::Module(module) => module.field_instantiation_count(idx.0),
            BinaryType::Script(_) => unreachable!("Scripts cannot have type instructions"),
        }
    }

    pub(crate) fn field_handle_to_struct(&self, idx: FieldHandleIndex) -> Type {
        match &self.binary {
            BinaryType::Module(module) => {
                let struct_ty = &module.field_handles[idx.0 as usize].definition_struct_type;
                self.loader()
                    .ty_builder()
                    .create_struct_ty(struct_ty.idx, AbilityInfo::struct_(struct_ty.abilities))
            }
            BinaryType::Script(_) => unreachable!("Scripts cannot have field instructions"),
        }
    }

    pub(crate) fn field_instantiation_to_struct(
        &self,
        idx: FieldInstantiationIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<Type> {
        match &self.binary {
            BinaryType::Module(module) => {
                let field_inst = &module.field_instantiations[idx.0 as usize];
                let struct_ty = &field_inst.definition_struct_type;
                let ty_params = &field_inst.instantiation;

                self.loader()
                    .ty_builder()
                    .create_struct_instantiation_ty(struct_ty, ty_params, ty_args)
            }
            BinaryType::Script(_) => unreachable!("Scripts cannot have field instructions"),
        }
    }

    // get the loader
    pub(crate) fn loader(&self) -> &Loader {
        self.loader
    }

    // get the loader
    pub(crate) fn module_store(&self) -> &ModuleStorageAdapter {
        self.module_store
    }
}

impl Loader {
    pub(crate) fn update_native_functions(
        &mut self,
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
    ) -> PartialVMResult<()> {
        self.natives = NativeFunctions::new(natives)?;
        Ok(())
    }
}
