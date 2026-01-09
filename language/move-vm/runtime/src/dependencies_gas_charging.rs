// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    data_cache::TransactionDataCache, loader::ModuleStorageAdapter,
    module_traversal::TraversalContext,
};
use move_binary_format::{
    access::ModuleAccess,
    errors::{Location, VMResult},
    CompiledModule,
};
use move_core_types::{
    account_address::AccountAddress, gas_algebra::NumBytes, identifier::IdentStr,
    language_storage::ModuleId, vm_status::StatusCode,
};
use move_vm_types::gas::DependencyGasMeter;
use std::sync::Arc;
use typed_arena::Arena;

/// Traverses the transitive closure of dependencies, starting from the specified modules,
/// and performs dependency gas metering.
///
/// The traversal is depth-first: module first, then dependencies, then friends.
pub(crate) fn check_dependencies_and_charge_gas<'a, I>(
    module_store: &ModuleStorageAdapter,
    data_store: &mut TransactionDataCache,
    gas_meter: &mut impl DependencyGasMeter,
    traversal_context: &mut TraversalContext<'a>,
    ids: I,
) -> VMResult<()>
where
    I: IntoIterator<Item = (&'a AccountAddress, &'a IdentStr)>,
    I::IntoIter: DoubleEndedIterator,
{
    let referenced_modules: &Arena<Arc<CompiledModule>> = traversal_context.referenced_modules;
    let mut stack = Vec::with_capacity(512);

    for (addr, name) in ids.into_iter().rev() {
        if traversal_context.visit_if_not_special_address(addr, name) {
            stack.push((addr, name, true));
        }
    }

    while let Some((addr, name, allow_loading_failure)) = stack.pop() {
        let (module, size) = match module_store.module_at_by_ref(addr, name) {
            Some(module) => (module.module.clone(), module.size),
            None => {
                let (module, size, _) = data_store.load_compiled_module_to_cache(
                    ModuleId::new(*addr, name.to_owned()),
                    allow_loading_failure,
                )?;
                (module, size)
            },
        };

        let module = referenced_modules.alloc(module);

        gas_meter
            .charge_dependency_for_existing(addr, name, NumBytes::new(size as u64))
            .map_err(|err| err.finish(Location::Module(ModuleId::new(*addr, name.to_owned()))))?;

        for (addr, name) in module
            .immediate_dependencies_iter()
            .chain(module.immediate_friends_iter())
            .rev()
        {
            if traversal_context.visit_if_not_special_address(addr, name) {
                stack.push((addr, name, false));
            }
        }
    }

    Ok(())
}

/// Similar to `check_dependencies_and_charge_gas`, except that this does not recurse
/// into transitive dependencies and allows non-existent modules.
pub(crate) fn check_dependencies_and_charge_gas_non_recursive_optional<'a, I>(
    module_store: &ModuleStorageAdapter,
    data_store: &mut TransactionDataCache,
    gas_meter: &mut impl DependencyGasMeter,
    traversal_context: &mut TraversalContext<'a>,
    ids: I,
) -> VMResult<()>
where
    I: IntoIterator<Item = (&'a AccountAddress, &'a IdentStr)>,
{
    for (addr, name) in ids.into_iter() {
        if !traversal_context.visit_if_not_special_address(addr, name) {
            continue;
        }

        let size = match module_store.module_at_by_ref(addr, name) {
            Some(module) => module.size,
            None => match data_store
                .load_compiled_module_to_cache(ModuleId::new(*addr, name.to_owned()), true)
            {
                Ok((_module, size, _hash)) => size,
                Err(err) if err.major_status() == StatusCode::LINKER_ERROR => continue,
                Err(err) => return Err(err),
            },
        };

        gas_meter
            .charge_dependency_for_existing(addr, name, NumBytes::new(size as u64))
            .map_err(|err| err.finish(Location::Module(ModuleId::new(*addr, name.to_owned()))))?;
    }

    Ok(())
}
