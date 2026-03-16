// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use lazy_static::lazy_static;
use parking_lot::Mutex;

pub(crate) struct VerifiedModuleCache(Mutex<lru::LruCache<[u8; 32], ()>>);

impl VerifiedModuleCache {
    const VERIFIED_CACHE_SIZE: usize = 100_000;

    pub(crate) fn empty() -> Self {
        Self(Mutex::new(lru::LruCache::new(Self::VERIFIED_CACHE_SIZE)))
    }

    pub(crate) fn contains(&self, module_hash: &[u8; 32]) -> bool {
        self.0.lock().get(module_hash).is_some()
    }

    pub(crate) fn put(&self, module_hash: [u8; 32]) {
        self.0.lock().put(module_hash, ());
    }
}

lazy_static! {
    pub(crate) static ref VERIFIED_MODULES_CACHE: VerifiedModuleCache =
        VerifiedModuleCache::empty();
}
