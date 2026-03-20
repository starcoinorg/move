// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::{
    errors::{PartialVMError, PartialVMResult},
    file_format::CompiledScript,
    CompiledModule,
};
use move_core_types::{
    account_address::AccountAddress, identifier::IdentStr, language_storage::ModuleId,
    vm_status::StatusCode,
};
use std::{collections::BTreeMap, sync::Arc};
use typed_arena::Arena;

pub struct TraversalStorage {
    referenced_scripts: Arena<Arc<CompiledScript>>,
    referenced_modules: Arena<Arc<CompiledModule>>,
    referenced_module_ids: Arena<ModuleId>,
    referenced_module_bundles: Arena<Vec<CompiledModule>>,
}

pub struct TraversalContext<'a> {
    pub visited: BTreeMap<(&'a AccountAddress, &'a IdentStr), ()>,

    pub referenced_scripts: &'a Arena<Arc<CompiledScript>>,
    pub referenced_modules: &'a Arena<Arc<CompiledModule>>,
    pub referenced_module_ids: &'a Arena<ModuleId>,
    pub referenced_module_bundles: &'a Arena<Vec<CompiledModule>>,
}

impl TraversalStorage {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            referenced_scripts: Arena::new(),
            referenced_modules: Arena::new(),
            referenced_module_ids: Arena::new(),
            referenced_module_bundles: Arena::new(),
        }
    }
}

impl<'a> TraversalContext<'a> {
    pub fn new(storage: &'a TraversalStorage) -> Self {
        Self {
            visited: BTreeMap::new(),

            referenced_scripts: &storage.referenced_scripts,
            referenced_modules: &storage.referenced_modules,
            referenced_module_ids: &storage.referenced_module_ids,
            referenced_module_bundles: &storage.referenced_module_bundles,
        }
    }

    pub fn visit_if_not_special_address(
        &mut self,
        addr: &'a AccountAddress,
        name: &'a IdentStr,
    ) -> bool {
        !addr.is_special() && self.visited.insert((addr, name), ()).is_none()
    }

    pub fn visit_if_not_special_module_id(&mut self, module_id: &ModuleId) -> bool {
        let addr = module_id.address();
        if addr.is_special() {
            return false;
        }

        let name = module_id.name();
        if self.visited.contains_key(&(addr, name)) {
            false
        } else {
            let module_id = self.referenced_module_ids.alloc(module_id.clone());
            self.visited
                .insert((module_id.address(), module_id.name()), ());
            true
        }
    }

    fn check_visited_impl(&self, addr: &AccountAddress, name: &IdentStr) -> PartialVMResult<()> {
        if self.visited.contains_key(&(addr, name)) {
            return Ok(());
        }

        let msg = format!("Module {}::{} has not been visited", addr, name);
        Err(PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR).with_message(msg))
    }

    pub fn check_is_special_or_visited(
        &self,
        addr: &AccountAddress,
        name: &IdentStr,
    ) -> PartialVMResult<()> {
        if addr.is_special() {
            return Ok(());
        }

        self.check_visited_impl(addr, name)
    }

    pub fn legacy_check_visited(
        &self,
        addr: &AccountAddress,
        name: &IdentStr,
    ) -> PartialVMResult<()> {
        self.check_visited_impl(addr, name)
    }

    pub(crate) fn push_next_ids_to_visit<I>(
        &mut self,
        stack: &mut Vec<(&'a AccountAddress, &'a IdentStr)>,
        ids: I,
    ) where
        I: IntoIterator<Item = (&'a AccountAddress, &'a IdentStr)>,
        I::IntoIter: DoubleEndedIterator,
    {
        for (addr, name) in ids.into_iter().rev() {
            if self.visit_if_not_special_address(addr, name) {
                stack.push((addr, name));
            }
        }
    }
}
