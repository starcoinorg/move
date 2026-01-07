// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::{file_format::CompiledScript, CompiledModule};
use move_core_types::{
    account_address::AccountAddress, identifier::IdentStr, language_storage::ModuleId,
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
        if addr.is_special() {
            return false;
        }
        self.visited.insert((addr, name), ()).is_none()
    }

    pub fn visit_if_not_special_module_id(&mut self, module_id: &'a ModuleId) -> bool {
        self.visit_if_not_special_address(module_id.address(), module_id.name())
    }

    pub fn is_visited(&self, addr: &'a AccountAddress, name: &'a IdentStr) -> bool {
        self.visited.contains_key(&(addr, name))
    }

    pub fn is_visited_module_id(&self, module_id: &'a ModuleId) -> bool {
        self.is_visited(module_id.address(), module_id.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use move_core_types::{account_address::AccountAddress, identifier::Identifier};

    #[test]
    fn visit_helpers_skip_special_addresses() {
        let storage = TraversalStorage::new();
        let mut context = TraversalContext::new(&storage);
        let name = IdentStr::new("M").unwrap();

        assert!(!context.visit_if_not_special_address(&AccountAddress::ONE, name));
        assert!(!context.is_visited(&AccountAddress::ONE, name));
    }

    #[test]
    fn visit_helpers_track_non_special_addresses() {
        let storage = TraversalStorage::new();
        let mut context = TraversalContext::new(&storage);
        let name = IdentStr::new("M").unwrap();
        let addr = AccountAddress::from_hex_literal("0x10").unwrap();

        assert!(context.visit_if_not_special_address(&addr, name));
        assert!(!context.visit_if_not_special_address(&addr, name));
        assert!(context.is_visited(&addr, name));

        let module_id = ModuleId::new(addr, Identifier::new("M").unwrap());
        assert!(!context.visit_if_not_special_module_id(&module_id));
        assert!(context.is_visited_module_id(&module_id));
    }
}
