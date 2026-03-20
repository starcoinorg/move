// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use lazy_static::lazy_static;
use move_bytecode_verifier::VerifierConfig;
use parking_lot::Mutex;
use sha3::{Digest, Sha3_256};

pub(crate) struct VerifiedModuleCache(Mutex<lru::LruCache<[u8; 32], ()>>);

impl VerifiedModuleCache {
    const VERIFIED_CACHE_SIZE: usize = 100_000;

    pub(crate) fn empty() -> Self {
        Self(Mutex::new(lru::LruCache::new(Self::VERIFIED_CACHE_SIZE)))
    }

    fn cache_key(module_hash: &[u8; 32], verifier_config: &VerifierConfig) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(module_hash);
        hasher.update(format!("{:?}", verifier_config));
        hasher.finalize().into()
    }

    pub(crate) fn contains(
        &self,
        module_hash: &[u8; 32],
        verifier_config: &VerifierConfig,
    ) -> bool {
        let cache_key = Self::cache_key(module_hash, verifier_config);
        self.0.lock().get(&cache_key).is_some()
    }

    pub(crate) fn put(&self, module_hash: [u8; 32], verifier_config: &VerifierConfig) {
        let cache_key = Self::cache_key(&module_hash, verifier_config);
        self.0.lock().put(cache_key, ());
    }
}

lazy_static! {
    pub(crate) static ref VERIFIED_MODULES_CACHE: VerifiedModuleCache =
        VerifiedModuleCache::empty();
}
