// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::errors::PartialVMResult;
use move_core_types::{language_storage::ModuleId, value::MoveTypeLayout};
use move_vm_types::{loaded_data::runtime_types::StructNameIndex, ty_interner::TypeVecId};
use std::collections::HashSet;
use triomphe::Arc as TriompheArc;

#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct DefiningModules {
    modules: HashSet<ModuleId>,
    seen_modules: Vec<ModuleId>,
}

impl DefiningModules {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            modules: HashSet::new(),
            seen_modules: vec![],
        }
    }

    #[allow(dead_code)]
    pub fn insert(&mut self, module_id: &ModuleId) {
        if !self.modules.contains(module_id) {
            self.modules.insert(module_id.clone());
            self.seen_modules.push(module_id.clone());
        }
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = &ModuleId> {
        self.seen_modules.iter()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LayoutCacheEntry {
    layout: TriompheArc<MoveTypeLayout>,
    modules: TriompheArc<DefiningModules>,
}

impl LayoutCacheEntry {
    #[allow(dead_code)]
    pub(crate) fn new(layout: TriompheArc<MoveTypeLayout>, modules: DefiningModules) -> Self {
        Self {
            layout,
            modules: TriompheArc::new(modules),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn unpack(self) -> (TriompheArc<MoveTypeLayout>, TriompheArc<DefiningModules>) {
        (self.layout, self.modules)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct StructKey {
    pub idx: StructNameIndex,
    pub ty_args_id: TypeVecId,
}

pub trait LayoutCache {
    fn get_struct_layout(&self, key: &StructKey) -> Option<LayoutCacheEntry>;

    fn store_struct_layout(&self, key: &StructKey, entry: LayoutCacheEntry) -> PartialVMResult<()>;
}

pub trait NoOpLayoutCache {}

impl<T> LayoutCache for T
where
    T: NoOpLayoutCache,
{
    fn get_struct_layout(&self, _key: &StructKey) -> Option<LayoutCacheEntry> {
        None
    }

    fn store_struct_layout(
        &self,
        _key: &StructKey,
        _entry: LayoutCacheEntry,
    ) -> PartialVMResult<()> {
        Ok(())
    }
}
