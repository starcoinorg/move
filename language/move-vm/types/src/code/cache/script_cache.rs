// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::code::Code;
use ambassador::delegatable_trait;
use crossbeam::utils::CachePadded;
use dashmap::DashMap;
use hashbrown::HashMap;
use std::{cell::RefCell, hash::Hash, ops::Deref, sync::Arc};

#[delegatable_trait]
pub trait ScriptCache {
    type Key: Eq + Hash + Clone;
    type Deserialized;
    type Verified;

    fn insert_deserialized_script(
        &self,
        key: Self::Key,
        deserialized_script: Self::Deserialized,
    ) -> Arc<Self::Deserialized>;

    fn insert_verified_script(
        &self,
        key: Self::Key,
        verified_script: Self::Verified,
    ) -> Arc<Self::Verified>;

    fn get_script(&self, key: &Self::Key) -> Option<Code<Self::Deserialized, Self::Verified>>;

    fn num_scripts(&self) -> usize;
}

pub struct UnsyncScriptCache<K, D, V> {
    script_cache: RefCell<HashMap<K, Code<D, V>>>,
}

impl<K, D, V> UnsyncScriptCache<K, D, V>
where
    K: Eq + Hash + Clone,
    V: Deref<Target = Arc<D>>,
{
    pub fn empty() -> Self {
        Self {
            script_cache: RefCell::new(HashMap::new()),
        }
    }
}

impl<K, D, V> ScriptCache for UnsyncScriptCache<K, D, V>
where
    K: Eq + Hash + Clone,
    V: Deref<Target = Arc<D>>,
{
    type Deserialized = D;
    type Key = K;
    type Verified = V;

    fn insert_deserialized_script(
        &self,
        key: Self::Key,
        deserialized_script: Self::Deserialized,
    ) -> Arc<Self::Deserialized> {
        use hashbrown::hash_map::Entry::*;

        match self.script_cache.borrow_mut().entry(key) {
            Occupied(entry) => entry.get().deserialized().clone(),
            Vacant(entry) => entry
                .insert(Code::from_deserialized(deserialized_script))
                .deserialized()
                .clone(),
        }
    }

    fn insert_verified_script(
        &self,
        key: Self::Key,
        verified_script: Self::Verified,
    ) -> Arc<Self::Verified> {
        use hashbrown::hash_map::Entry::*;

        match self.script_cache.borrow_mut().entry(key) {
            Occupied(mut entry) => {
                if !entry.get().is_verified() {
                    let new_script = Code::from_verified(verified_script);
                    let verified_script = new_script.verified().clone();
                    entry.insert(new_script);
                    verified_script
                } else {
                    entry.get().verified().clone()
                }
            },
            Vacant(entry) => entry
                .insert(Code::from_verified(verified_script))
                .verified()
                .clone(),
        }
    }

    fn get_script(&self, key: &Self::Key) -> Option<Code<Self::Deserialized, Self::Verified>> {
        self.script_cache.borrow().get(key).cloned()
    }

    fn num_scripts(&self) -> usize {
        self.script_cache.borrow().len()
    }
}

pub struct SyncScriptCache<K, D, V> {
    script_cache: DashMap<K, CachePadded<Code<D, V>>>,
}

impl<K, D, V> SyncScriptCache<K, D, V>
where
    K: Eq + Hash + Clone,
    V: Deref<Target = Arc<D>>,
{
    pub fn empty() -> Self {
        Self {
            script_cache: DashMap::new(),
        }
    }
}

impl<K, D, V> ScriptCache for SyncScriptCache<K, D, V>
where
    K: Eq + Hash + Clone,
    V: Deref<Target = Arc<D>>,
{
    type Deserialized = D;
    type Key = K;
    type Verified = V;

    fn insert_deserialized_script(
        &self,
        key: Self::Key,
        deserialized_script: Self::Deserialized,
    ) -> Arc<Self::Deserialized> {
        use dashmap::mapref::entry::Entry::*;

        match self.script_cache.entry(key) {
            Occupied(entry) => entry.get().deserialized().clone(),
            Vacant(entry) => entry
                .insert(CachePadded::new(Code::from_deserialized(
                    deserialized_script,
                )))
                .deserialized()
                .clone(),
        }
    }

    fn insert_verified_script(
        &self,
        key: Self::Key,
        verified_script: Self::Verified,
    ) -> Arc<Self::Verified> {
        use dashmap::mapref::entry::Entry::*;

        match self.script_cache.entry(key) {
            Occupied(mut entry) => {
                if !entry.get().is_verified() {
                    let new_script = Code::from_verified(verified_script);
                    let verified_script = new_script.verified().clone();
                    entry.insert(CachePadded::new(new_script));
                    verified_script
                } else {
                    entry.get().verified().clone()
                }
            },
            Vacant(entry) => entry
                .insert(CachePadded::new(Code::from_verified(verified_script)))
                .verified()
                .clone(),
        }
    }

    fn get_script(&self, key: &Self::Key) -> Option<Code<Self::Deserialized, Self::Verified>> {
        let script = &**self.script_cache.get(key)?;
        Some(script.clone())
    }

    fn num_scripts(&self) -> usize {
        self.script_cache.len()
    }
}
