// Copyright (c) Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use parking_lot::RwLock;
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

/// Safe concurrent interner keyed by owned values.
pub struct ConcurrentBTreeInterner<T: 'static> {
    inner: RwLock<InternerPool<T>>,
}

struct InternerPool<T: 'static> {
    map: BTreeMap<T, usize>,
    vec: Vec<Arc<T>>,
}

impl<T> InternerPool<T> {
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            vec: Vec::new(),
        }
    }
}

impl<T> Default for InternerPool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> InternerPool<T>
where
    T: Ord,
{
    fn flush(&mut self) {
        self.map.clear();
        self.vec.clear();
    }
}

impl<T> ConcurrentBTreeInterner<T> {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(InternerPool::new()),
        }
    }
}

impl<T> Default for ConcurrentBTreeInterner<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ConcurrentBTreeInterner<T>
where
    T: Clone + Ord,
{
    pub fn intern(&self, val: T) -> usize {
        self.intern_deferred(Cow::Owned(val))
    }

    pub fn intern_by_ref(&self, val: &T) -> usize {
        self.intern_deferred(Cow::Borrowed(val))
    }

    pub fn intern_deferred(&self, val: Cow<T>) -> usize {
        {
            let inner = self.inner.read();
            if let Some(idx) = inner.map.get(val.as_ref()) {
                return *idx;
            }
        }

        let val = val.into_owned();
        let mut inner = self.inner.write();
        if let Some(idx) = inner.map.get(&val) {
            return *idx;
        }

        let idx = inner.vec.len();
        inner.vec.push(Arc::new(val.clone()));
        inner.map.insert(val, idx);
        idx
    }

    pub fn len(&self) -> usize {
        self.inner.read().vec.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().vec.is_empty()
    }

    pub fn lookup(&self, val: &T) -> Option<usize> {
        self.inner.read().map.get(val).cloned()
    }
    pub fn flush(&self) {
        self.inner.write().flush();
    }
}
