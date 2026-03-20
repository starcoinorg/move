// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::code::Code;
use ambassador::delegatable_trait;
use crossbeam::utils::CachePadded;
use dashmap::DashMap;
use hashbrown::HashMap;
use move_binary_format::errors::VMResult;
use std::{cell::RefCell, cmp::Ordering, hash::Hash, mem, ops::Deref, sync::Arc};

/// Represents module code stored in [ModuleCode].
pub struct ModuleCode<DC, VC, E> {
    code: Code<DC, VC>,
    extension: Arc<E>,
}

impl<DC, VC, E> ModuleCode<DC, VC, E>
where
    VC: Deref<Target = Arc<DC>>,
{
    pub fn from_deserialized(deserialized_code: DC, extension: Arc<E>) -> Self {
        Self {
            code: Code::from_deserialized(deserialized_code),
            extension,
        }
    }

    pub fn from_verified(verified_code: VC, extension: Arc<E>) -> Self {
        Self::from_arced_verified(Arc::new(verified_code), extension)
    }

    pub fn from_arced_verified(verified_code: Arc<VC>, extension: Arc<E>) -> Self {
        Self {
            code: Code::from_arced_verified(verified_code),
            extension,
        }
    }

    pub fn code(&self) -> &Code<DC, VC> {
        &self.code
    }

    pub fn extension(&self) -> &Arc<E> {
        &self.extension
    }
}

impl<DC, VC, E> Clone for ModuleCode<DC, VC, E> {
    fn clone(&self) -> Self {
        Self {
            code: self.code.clone(),
            extension: self.extension.clone(),
        }
    }
}

pub trait ModuleCodeBuilder {
    type Key: Eq + Hash + Clone;
    type Deserialized;
    type Verified;
    type Extension;

    fn build(
        &self,
        key: &Self::Key,
    ) -> VMResult<Option<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>>;
}

#[delegatable_trait]
pub trait ModuleCache {
    type Key: Eq + Hash + Clone;
    type Deserialized;
    type Verified;
    type Extension;
    type Version: Clone + Default + Ord;

    fn insert_deserialized_module(
        &self,
        key: Self::Key,
        deserialized_code: Self::Deserialized,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>>;

    fn insert_verified_module(
        &self,
        key: Self::Key,
        verified_code: Self::Verified,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>>;

    fn get_module_or_build_with(
        &self,
        key: &Self::Key,
        builder: &dyn ModuleCodeBuilder<
            Key = Self::Key,
            Deserialized = Self::Deserialized,
            Verified = Self::Verified,
            Extension = Self::Extension,
        >,
    ) -> VMResult<
        Option<(
            Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>,
            Self::Version,
        )>,
    >;

    fn num_modules(&self) -> usize;
}

struct VersionedModuleCode<DC, VC, E, V> {
    module_code: Arc<ModuleCode<DC, VC, E>>,
    version: V,
}

impl<DC, VC, E, V> VersionedModuleCode<DC, VC, E, V>
where
    V: Default + Clone + Ord,
{
    fn new(module_code: ModuleCode<DC, VC, E>, version: V) -> Self {
        Self {
            module_code: Arc::new(module_code),
            version,
        }
    }

    fn new_with_default_version(module_code: ModuleCode<DC, VC, E>) -> Self {
        Self::new(module_code, V::default())
    }

    fn module_code(&self) -> &Arc<ModuleCode<DC, VC, E>> {
        &self.module_code
    }

    fn into_module_code(self) -> Arc<ModuleCode<DC, VC, E>> {
        self.module_code
    }

    fn version(&self) -> V {
        self.version.clone()
    }

    fn as_module_code_and_version(&self) -> (Arc<ModuleCode<DC, VC, E>>, V) {
        (self.module_code.clone(), self.version.clone())
    }
}

impl<DC, VC, E, V> Clone for VersionedModuleCode<DC, VC, E, V>
where
    V: Default + Clone + Ord,
{
    fn clone(&self) -> Self {
        Self {
            module_code: self.module_code.clone(),
            version: self.version.clone(),
        }
    }
}

macro_rules! version_too_small_error {
    () => {
        move_binary_format::errors::PartialVMError::new(
            move_core_types::vm_status::StatusCode::SPECULATIVE_EXECUTION_ABORT_ERROR,
        )
        .with_message("Trying to insert smaller version that exists in module cache".to_string())
        .finish(move_binary_format::errors::Location::Undefined)
    };
}

/// Non-[Sync] version of module cache suitable for sequential execution.
pub struct UnsyncModuleCache<K, DC, VC, E, V> {
    module_cache: RefCell<HashMap<K, VersionedModuleCode<DC, VC, E, V>>>,
}

impl<K, DC, VC, E, V> UnsyncModuleCache<K, DC, VC, E, V>
where
    K: Eq + Hash + Clone,
    VC: Deref<Target = Arc<DC>>,
    V: Clone + Default + Ord,
{
    pub fn empty() -> Self {
        Self {
            module_cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn into_modules_iter(self) -> impl Iterator<Item = (K, Arc<ModuleCode<DC, VC, E>>)> {
        self.module_cache
            .into_inner()
            .into_iter()
            .map(|(k, m)| (k, m.into_module_code()))
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn get_module_version(&self, key: &K) -> Option<V> {
        self.module_cache.borrow().get(key).map(|m| m.version())
    }
}

impl<K, DC, VC, E, V> ModuleCache for UnsyncModuleCache<K, DC, VC, E, V>
where
    K: Eq + Hash + Clone,
    VC: Deref<Target = Arc<DC>>,
    V: Clone + Default + Ord,
{
    type Deserialized = DC;
    type Extension = E;
    type Key = K;
    type Verified = VC;
    type Version = V;

    fn insert_deserialized_module(
        &self,
        key: Self::Key,
        deserialized_code: Self::Deserialized,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>> {
        use hashbrown::hash_map::Entry::*;

        let mut cache = self.module_cache.borrow_mut();
        match cache.entry(key) {
            Occupied(entry) => match version.cmp(&entry.get().version()) {
                Ordering::Less => Err(version_too_small_error!()),
                Ordering::Equal | Ordering::Greater => Ok(entry.get().module_code().clone()),
            },
            Vacant(entry) => {
                let module = ModuleCode::from_deserialized(deserialized_code, extension);
                Ok(entry
                    .insert(VersionedModuleCode::new(module, version))
                    .module_code()
                    .clone())
            }
        }
    }

    fn insert_verified_module(
        &self,
        key: Self::Key,
        verified_code: Self::Verified,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>> {
        use hashbrown::hash_map::Entry::*;

        let mut cache = self.module_cache.borrow_mut();
        match cache.entry(key) {
            Occupied(mut entry) => match version.cmp(&entry.get().version()) {
                Ordering::Less => Err(version_too_small_error!()),
                Ordering::Equal if entry.get().module_code().code().is_verified() => {
                    Ok(entry.get().module_code().clone())
                }
                Ordering::Equal | Ordering::Greater => {
                    let module = ModuleCode::from_verified(verified_code, extension);
                    let module_code = VersionedModuleCode::new(module, version);
                    let prev = mem::replace(entry.get_mut(), module_code);
                    debug_assert!(
                        prev.version() <= entry.get().version(),
                        "New module code version should not be smaller than old version",
                    );
                    Ok(entry.get().module_code().clone())
                }
            },
            Vacant(entry) => {
                let module = ModuleCode::from_verified(verified_code, extension);
                Ok(entry
                    .insert(VersionedModuleCode::new(module, version))
                    .module_code()
                    .clone())
            }
        }
    }

    fn get_module_or_build_with(
        &self,
        key: &Self::Key,
        builder: &dyn ModuleCodeBuilder<
            Key = Self::Key,
            Deserialized = Self::Deserialized,
            Verified = Self::Verified,
            Extension = Self::Extension,
        >,
    ) -> VMResult<
        Option<(
            Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>,
            Self::Version,
        )>,
    > {
        use hashbrown::hash_map::Entry::*;

        if let Some(module) = self.module_cache.borrow().get(key) {
            return Ok(Some(module.as_module_code_and_version()));
        }

        let initialized_module = match builder.build(key)? {
            Some(module) => module,
            None => return Ok(None),
        };

        let mut cache = self.module_cache.borrow_mut();
        Ok(Some(
            match cache.entry(key.clone()) {
                Occupied(entry) => entry.get().clone(),
                Vacant(entry) => entry
                    .insert(VersionedModuleCode::new_with_default_version(
                        initialized_module,
                    ))
                    .clone(),
            }
            .as_module_code_and_version(),
        ))
    }

    fn num_modules(&self) -> usize {
        self.module_cache.borrow().len()
    }
}

/// [Sync] version of module cache suitable for parallel execution.
pub struct SyncModuleCache<K, DC, VC, E, V> {
    module_cache: DashMap<K, CachePadded<VersionedModuleCode<DC, VC, E, V>>>,
}

impl<K, DC, VC, E, V> SyncModuleCache<K, DC, VC, E, V>
where
    K: Eq + Hash + Clone,
    VC: Deref<Target = Arc<DC>>,
    V: Clone + Default + Ord,
{
    pub fn empty() -> Self {
        Self {
            module_cache: DashMap::new(),
        }
    }
}

impl<K, DC, VC, E, V> ModuleCache for SyncModuleCache<K, DC, VC, E, V>
where
    K: Eq + Hash + Clone,
    VC: Deref<Target = Arc<DC>>,
    V: Clone + Default + Ord,
{
    type Deserialized = DC;
    type Extension = E;
    type Key = K;
    type Verified = VC;
    type Version = V;

    fn insert_deserialized_module(
        &self,
        key: Self::Key,
        deserialized_code: Self::Deserialized,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>> {
        use dashmap::mapref::entry::Entry::*;

        match self.module_cache.entry(key) {
            Occupied(entry) => match version.cmp(&entry.get().version()) {
                Ordering::Less => Err(version_too_small_error!()),
                Ordering::Equal | Ordering::Greater => Ok(entry.get().module_code().clone()),
            },
            Vacant(entry) => {
                let module = ModuleCode::from_deserialized(deserialized_code, extension);
                Ok(entry
                    .insert(CachePadded::new(VersionedModuleCode::new(module, version)))
                    .module_code()
                    .clone())
            }
        }
    }

    fn insert_verified_module(
        &self,
        key: Self::Key,
        verified_code: Self::Verified,
        extension: Arc<Self::Extension>,
        version: Self::Version,
    ) -> VMResult<Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>> {
        use dashmap::mapref::entry::Entry::*;

        match self.module_cache.entry(key) {
            Occupied(mut entry) => match version.cmp(&entry.get().version()) {
                Ordering::Less => Err(version_too_small_error!()),
                Ordering::Equal if entry.get().module_code().code().is_verified() => {
                    Ok(entry.get().module_code().clone())
                }
                Ordering::Equal | Ordering::Greater => {
                    let module = ModuleCode::from_verified(verified_code, extension);
                    let module_code = VersionedModuleCode::new(module, version);
                    let prev = mem::replace(&mut *entry.get_mut(), CachePadded::new(module_code));
                    debug_assert!(
                        prev.version() <= entry.get().version(),
                        "New module code version should not be smaller than old version",
                    );
                    Ok(entry.get().module_code().clone())
                }
            },
            Vacant(entry) => {
                let module = ModuleCode::from_verified(verified_code, extension);
                Ok(entry
                    .insert(CachePadded::new(VersionedModuleCode::new(module, version)))
                    .module_code()
                    .clone())
            }
        }
    }

    fn get_module_or_build_with(
        &self,
        key: &Self::Key,
        builder: &dyn ModuleCodeBuilder<
            Key = Self::Key,
            Deserialized = Self::Deserialized,
            Verified = Self::Verified,
            Extension = Self::Extension,
        >,
    ) -> VMResult<
        Option<(
            Arc<ModuleCode<Self::Deserialized, Self::Verified, Self::Extension>>,
            Self::Version,
        )>,
    > {
        use dashmap::mapref::entry::Entry::*;

        if let Some(module) = self.module_cache.get(key) {
            return Ok(Some(module.as_module_code_and_version()));
        }

        let initialized_module = match builder.build(key)? {
            Some(module) => module,
            None => return Ok(None),
        };

        Ok(Some(
            match self.module_cache.entry(key.clone()) {
                Occupied(entry) => entry.get().clone(),
                Vacant(entry) => entry
                    .insert(CachePadded::new(
                        VersionedModuleCode::new_with_default_version(initialized_module),
                    ))
                    .clone(),
            }
            .as_module_code_and_version(),
        ))
    }

    fn num_modules(&self) -> usize {
        self.module_cache.len()
    }
}
