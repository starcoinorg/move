// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::loaded_data::runtime_types::{StructIdentifier, StructNameIndex};
use move_binary_format::errors::PartialVMResult;
use move_core_types::language_storage::{StructTag, TypeTag};
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

macro_rules! panic_error {
    ($msg:expr) => {{
        move_binary_format::errors::PartialVMError::new(
            move_core_types::vm_status::StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR,
        )
        .with_message(format!("Panic detected: {:?}", $msg))
    }};
}

#[derive(Clone)]
struct IndexMap<T: Clone + Ord> {
    forward_map: BTreeMap<T, usize>,
    backward_map: Vec<Arc<T>>,
}

/// A data structure to cache struct identifiers (address, module name, struct name) and use
/// indices instead.
pub struct StructNameIndexMap(RwLock<IndexMap<StructIdentifier>>);

impl StructNameIndexMap {
    pub fn empty() -> Self {
        Self(RwLock::new(IndexMap {
            forward_map: BTreeMap::new(),
            backward_map: vec![],
        }))
    }

    pub fn flush(&self) {
        let mut index_map = self.0.write();
        index_map.backward_map.clear();
        index_map.forward_map.clear();
    }

    pub fn struct_name_to_idx(
        &self,
        struct_name: &StructIdentifier,
    ) -> PartialVMResult<StructNameIndex> {
        {
            let index_map = self.0.read();
            if let Some(idx) = index_map.forward_map.get(struct_name) {
                return Ok(StructNameIndex(*idx));
            }
        }

        let forward_key = struct_name.clone();
        let backward_value = Arc::new(struct_name.clone());

        let idx = {
            let mut index_map = self.0.write();
            if let Some(idx) = index_map.forward_map.get(struct_name) {
                return Ok(StructNameIndex(*idx));
            }

            let idx = index_map.backward_map.len();
            index_map.backward_map.push(backward_value);
            index_map.forward_map.insert(forward_key, idx);
            idx
        };

        Ok(StructNameIndex(idx))
    }

    fn idx_to_struct_name_helper<'a>(
        index_map: &'a parking_lot::RwLockReadGuard<IndexMap<StructIdentifier>>,
        idx: StructNameIndex,
    ) -> PartialVMResult<&'a Arc<StructIdentifier>> {
        index_map.backward_map.get(idx.0).ok_or_else(|| {
            let msg = format!(
                "Index out of bounds when accessing struct name reference at index {}, backward map length: {}",
                idx.0,
                index_map.backward_map.len()
            );
            panic_error!(msg)
        })
    }

    pub fn idx_to_struct_name_ref(
        &self,
        idx: StructNameIndex,
    ) -> PartialVMResult<Arc<StructIdentifier>> {
        let index_map = self.0.read();
        Ok(Self::idx_to_struct_name_helper(&index_map, idx)?.clone())
    }

    pub fn idx_to_struct_name(&self, idx: StructNameIndex) -> PartialVMResult<StructIdentifier> {
        let index_map = self.0.read();
        Ok(Self::idx_to_struct_name_helper(&index_map, idx)?
            .as_ref()
            .clone())
    }

    pub fn idx_to_struct_tag(
        &self,
        idx: StructNameIndex,
        ty_args: Vec<TypeTag>,
    ) -> PartialVMResult<StructTag> {
        let index_map = self.0.read();
        let struct_name = Self::idx_to_struct_name_helper(&index_map, idx)?.as_ref();
        Ok(StructTag {
            address: *struct_name.module.address(),
            module: struct_name.module.name().to_owned(),
            name: struct_name.name.clone(),
            type_args: ty_args,
        })
    }

    pub fn checked_len(&self) -> PartialVMResult<usize> {
        let (forward_map_len, backward_map_len) = {
            let index_map = self.0.read();
            (index_map.forward_map.len(), index_map.backward_map.len())
        };

        if forward_map_len != backward_map_len {
            let msg = format!(
                "Indexed map size mismatch: forward map has length {}, but backward map has length {}",
                forward_map_len, backward_map_len
            );
            return Err(panic_error!(msg));
        }

        Ok(forward_map_len)
    }
}
