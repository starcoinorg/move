// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use dashmap::DashMap;
use move_core_types::value::{MoveStructLayout, MoveTypeLayout};
use once_cell::sync::Lazy;
use rustc_hash::FxHasher;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{Hash, Hasher},
};

type LayoutBucket = Vec<(MoveTypeLayout, bool)>;
const GLOBAL_CACHE_SOFT_LIMIT: usize = 100_000;

static GLOBAL_LAYOUT_IDENTIFIER_MAPPING_CACHE: Lazy<DashMap<u64, LayoutBucket>> =
    Lazy::new(DashMap::new);

#[inline]
fn hash_layout(layout: &MoveTypeLayout) -> u64 {
    let mut hasher = FxHasher::default();
    layout.hash(&mut hasher);
    hasher.finish()
}

#[inline]
fn lookup_bucket(bucket: &LayoutBucket, layout: &MoveTypeLayout) -> Option<bool> {
    bucket
        .iter()
        .find(|(cached_layout, _)| cached_layout == layout)
        .map(|(_, value)| *value)
}

#[inline]
fn insert_bucket(bucket: &mut LayoutBucket, layout: &MoveTypeLayout, value: bool) {
    if lookup_bucket(bucket, layout).is_none() {
        bucket.push((layout.clone(), value));
    }
}

#[inline]
fn maybe_trim_global_cache() {
    if GLOBAL_LAYOUT_IDENTIFIER_MAPPING_CACHE.len() > GLOBAL_CACHE_SOFT_LIMIT {
        GLOBAL_LAYOUT_IDENTIFIER_MAPPING_CACHE.clear();
    }
}

pub fn compute_layout_has_identifier_mappings(layout: &MoveTypeLayout) -> bool {
    match layout {
        MoveTypeLayout::Native(..) => true,
        MoveTypeLayout::Vector(inner) => compute_layout_has_identifier_mappings(inner),
        MoveTypeLayout::Struct(struct_layout) => match struct_layout {
            MoveStructLayout::Runtime(fields) => {
                fields.iter().any(compute_layout_has_identifier_mappings)
            }
            MoveStructLayout::WithFields(fields) => fields
                .iter()
                .any(|field| compute_layout_has_identifier_mappings(&field.layout)),
            MoveStructLayout::WithTypes { fields, .. } => fields
                .iter()
                .any(|field| compute_layout_has_identifier_mappings(&field.layout)),
        },
        _ => false,
    }
}

/// Per-view cache for checking whether a type layout contains delayed-field identifier mappings.
///
/// Caching strategy:
/// 1. Local cache keyed by stable hash + equality bucket.
/// 2. Process-wide global cache keyed by stable hash + equality bucket.
/// 3. Optional stable-reference fast path (`has_identifier_mappings_stable_ref`) for callers that
///    can guarantee layout reference stability.
#[derive(Default)]
pub struct LayoutIdentifierMappingCache {
    last_layout_ptr: Cell<usize>,
    last_value: Cell<bool>,
    has_last: Cell<bool>,
    entries: RefCell<HashMap<u64, LayoutBucket>>,
}

impl LayoutIdentifierMappingCache {
    pub fn has_identifier_mappings(&self, layout: &MoveTypeLayout) -> bool {
        let key = hash_layout(layout);
        if let Some(cached) = self
            .entries
            .borrow()
            .get(&key)
            .and_then(|bucket| lookup_bucket(bucket, layout))
        {
            return cached;
        }

        if let Some(cached) = GLOBAL_LAYOUT_IDENTIFIER_MAPPING_CACHE
            .get(&key)
            .and_then(|bucket| lookup_bucket(bucket.value(), layout))
        {
            self.entries
                .borrow_mut()
                .entry(key)
                .and_modify(|bucket| insert_bucket(bucket, layout, cached))
                .or_insert_with(|| vec![(layout.clone(), cached)]);
            return cached;
        }

        let computed = compute_layout_has_identifier_mappings(layout);
        self.entries
            .borrow_mut()
            .entry(key)
            .and_modify(|bucket| insert_bucket(bucket, layout, computed))
            .or_insert_with(|| vec![(layout.clone(), computed)]);
        GLOBAL_LAYOUT_IDENTIFIER_MAPPING_CACHE
            .entry(key)
            .and_modify(|bucket| insert_bucket(bucket, layout, computed))
            .or_insert_with(|| vec![(layout.clone(), computed)]);
        maybe_trim_global_cache();
        computed
    }

    /// Fast path for callers that can guarantee the layout reference is stable across checks.
    /// For general callers, use `has_identifier_mappings`.
    pub fn has_identifier_mappings_stable_ref(&self, layout: &MoveTypeLayout) -> bool {
        let ptr = layout as *const MoveTypeLayout as usize;
        if self.has_last.get() && self.last_layout_ptr.get() == ptr {
            return self.last_value.get();
        }
        let result = self.has_identifier_mappings(layout);
        self.last_layout_ptr.set(ptr);
        self.last_value.set(result);
        self.has_last.set(true);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use move_core_types::{
        identifier::Identifier,
        language_storage::StructTag,
        value::{IdentifierMappingKind, MoveFieldLayout},
    };

    fn id(name: &str) -> Identifier {
        Identifier::new(name).unwrap()
    }

    fn runtime_native_layout() -> MoveTypeLayout {
        MoveTypeLayout::Struct(MoveStructLayout::Runtime(vec![
            MoveTypeLayout::U64,
            MoveTypeLayout::Native(
                IdentifierMappingKind::Aggregator,
                Box::new(MoveTypeLayout::U128),
            ),
        ]))
    }

    fn with_fields_native_layout() -> MoveTypeLayout {
        MoveTypeLayout::Struct(MoveStructLayout::WithFields(vec![
            MoveFieldLayout::new(id("a"), MoveTypeLayout::U64),
            MoveFieldLayout::new(
                id("b"),
                MoveTypeLayout::Native(
                    IdentifierMappingKind::Snapshot,
                    Box::new(MoveTypeLayout::U128),
                ),
            ),
        ]))
    }

    fn with_types_native_layout() -> MoveTypeLayout {
        MoveTypeLayout::Struct(MoveStructLayout::WithTypes {
            type_: StructTag {
                address: move_core_types::account_address::AccountAddress::ONE,
                module: id("M"),
                name: id("S"),
                type_args: vec![],
            },
            fields: vec![
                MoveFieldLayout::new(id("x"), MoveTypeLayout::U8),
                MoveFieldLayout::new(
                    id("y"),
                    MoveTypeLayout::Native(
                        IdentifierMappingKind::DerivedString,
                        Box::new(MoveTypeLayout::U64),
                    ),
                ),
            ],
        })
    }

    fn all_test_layouts() -> Vec<MoveTypeLayout> {
        vec![
            MoveTypeLayout::U64,
            runtime_native_layout(),
            with_fields_native_layout(),
            with_types_native_layout(),
            MoveTypeLayout::Vector(Box::new(MoveTypeLayout::Bool)),
        ]
    }

    #[test]
    fn test_compute_layout_has_identifier_mappings() {
        assert!(!compute_layout_has_identifier_mappings(
            &MoveTypeLayout::U64
        ));
        assert!(compute_layout_has_identifier_mappings(
            &runtime_native_layout()
        ));
        assert!(compute_layout_has_identifier_mappings(
            &with_fields_native_layout()
        ));
        assert!(compute_layout_has_identifier_mappings(
            &with_types_native_layout()
        ));
    }

    #[test]
    fn test_layout_identifier_mapping_cache_matches_compute() {
        let cache = LayoutIdentifierMappingCache::default();
        let layouts = all_test_layouts();

        for layout in layouts {
            let expected = compute_layout_has_identifier_mappings(&layout);
            let cached_first = cache.has_identifier_mappings(&layout);
            let cached_second = cache.has_identifier_mappings(&layout);
            assert_eq!(expected, cached_first);
            assert_eq!(cached_first, cached_second);
        }
    }

    #[test]
    fn test_layout_identifier_mapping_cache_stable_ref_matches_legacy_path() {
        let cache = LayoutIdentifierMappingCache::default();
        let layouts = all_test_layouts();

        for layout in &layouts {
            let expected = compute_layout_has_identifier_mappings(layout);
            for _ in 0..16 {
                assert_eq!(expected, cache.has_identifier_mappings_stable_ref(layout));
            }
            assert_eq!(expected, cache.has_identifier_mappings(layout));
        }
    }

    #[test]
    fn test_layout_identifier_mapping_cache_stable_ref_mixed_with_fresh_layouts() {
        let cache = LayoutIdentifierMappingCache::default();
        let baseline_sequence = vec![
            MoveTypeLayout::U64,
            runtime_native_layout(),
            MoveTypeLayout::U64,
            with_fields_native_layout(),
            MoveTypeLayout::Vector(Box::new(MoveTypeLayout::Bool)),
            with_types_native_layout(),
            runtime_native_layout(),
            MoveTypeLayout::U64,
        ];

        for layout in &baseline_sequence {
            let expected = compute_layout_has_identifier_mappings(layout);
            assert_eq!(expected, cache.has_identifier_mappings_stable_ref(layout));
            assert_eq!(expected, cache.has_identifier_mappings(layout));
        }
    }

    #[test]
    fn test_layout_identifier_mapping_cache_cross_instance_consistency() {
        let layout = with_types_native_layout();
        let cache_a = LayoutIdentifierMappingCache::default();
        let cache_b = LayoutIdentifierMappingCache::default();

        let expected = compute_layout_has_identifier_mappings(&layout);
        assert_eq!(expected, cache_a.has_identifier_mappings(&layout));
        assert_eq!(expected, cache_b.has_identifier_mappings(&layout));
    }
}
