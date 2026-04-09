// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_core_types::value::{MoveStructLayout, MoveTypeLayout};
use std::{
    cell::RefCell,
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    hash::{Hash, Hasher},
};

type LayoutBucket = Vec<(MoveTypeLayout, bool)>;

#[inline]
fn hash_layout(layout: &MoveTypeLayout) -> u64 {
    let mut hasher = DefaultHasher::new();
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
/// This uses stable hash + equality matching to avoid pointer-reuse hazards.
#[derive(Default)]
pub struct LayoutIdentifierMappingCache {
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

        let computed = compute_layout_has_identifier_mappings(layout);
        self.entries
            .borrow_mut()
            .entry(key)
            .and_modify(|bucket| insert_bucket(bucket, layout, computed))
            .or_insert_with(|| vec![(layout.clone(), computed)]);
        computed
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
        let layouts = vec![
            MoveTypeLayout::U64,
            runtime_native_layout(),
            with_fields_native_layout(),
            with_types_native_layout(),
            MoveTypeLayout::Vector(Box::new(MoveTypeLayout::Bool)),
        ];

        for layout in layouts {
            let expected = compute_layout_has_identifier_mappings(&layout);
            let cached_first = cache.has_identifier_mappings(&layout);
            let cached_second = cache.has_identifier_mappings(&layout);
            assert_eq!(expected, cached_first);
            assert_eq!(cached_first, cached_second);
        }
    }
}
