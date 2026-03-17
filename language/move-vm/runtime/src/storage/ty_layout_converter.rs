// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    module_traversal::TraversalContext,
    storage::{loader::traits::StructDefinitionLoader, ty_tag_converter::TypeTagConverter},
    RuntimeEnvironment,
};
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::{
    account_address::AccountAddress,
    ident_str,
    value::{IdentifierMappingKind, MoveFieldLayout, MoveStructLayout, MoveTypeLayout},
    vm_status::StatusCode,
};
use move_vm_types::{
    gas::{GasMeter, UnmeteredGasMeter},
    loaded_data::runtime_types::{StructIdentifier, StructNameIndex, Type},
};
use hashbrown::HashMap;
use std::{cell::RefCell, hash::{Hash, Hasher}};

pub const VALUE_DEPTH_MAX: u64 = 128;
const MAX_TYPE_TO_LAYOUT_NODES: u64 = 1536;

#[derive(Clone, Eq, PartialEq)]
struct StructLayoutKey {
    idx: StructNameIndex,
    ty_args: Vec<Type>,
}

impl Hash for StructLayoutKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
        self.ty_args.hash(state);
    }
}

pub struct LayoutConverter<'a, T> {
    struct_definition_loader: &'a T,
    layout_cache: RefCell<HashMap<StructLayoutKey, (MoveTypeLayout, bool)>>,
    annotated_layout_cache: RefCell<HashMap<StructLayoutKey, MoveTypeLayout>>,
}

impl<'a, T> LayoutConverter<'a, T>
where
    T: StructDefinitionLoader,
{
    pub(crate) fn new(struct_definition_loader: &'a T) -> Self {
        Self {
            struct_definition_loader,
            layout_cache: RefCell::new(HashMap::new()),
            annotated_layout_cache: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn runtime_environment(&self) -> &RuntimeEnvironment {
        self.struct_definition_loader.runtime_environment()
    }

    pub(crate) fn is_lazy_loading_enabled(&self) -> bool {
        self.struct_definition_loader.is_lazy_loading_enabled()
    }

    pub(crate) fn type_to_type_layout_with_identifier_mappings(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
    ) -> PartialVMResult<(MoveTypeLayout, bool)> {
        let mut count = 0;
        self.type_to_type_layout_impl(gas_meter, traversal_context, ty, &mut count, 1)
    }

    pub(crate) fn type_to_type_layout(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
    ) -> PartialVMResult<MoveTypeLayout> {
        let (layout, _) =
            self.type_to_type_layout_with_identifier_mappings(gas_meter, traversal_context, ty)?;
        Ok(layout)
    }

    pub(crate) fn type_to_fully_annotated_layout(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
    ) -> PartialVMResult<MoveTypeLayout> {
        let mut count = 0;
        self.type_to_fully_annotated_layout_impl(gas_meter, traversal_context, ty, &mut count, 1)
    }

    fn get_identifier_mapping_kind(&self, struct_name: &StructIdentifier) -> Option<IdentifierMappingKind> {
        if !self.runtime_environment().vm_config().aggregator_v2_type_tagging {
            return None;
        }

        let ident_str_to_kind = |ident_str: &move_core_types::identifier::IdentStr| -> Option<IdentifierMappingKind> {
            if ident_str.eq(ident_str!("Aggregator")) {
                Some(IdentifierMappingKind::Aggregator)
            } else if ident_str.eq(ident_str!("AggregatorSnapshot")) {
                Some(IdentifierMappingKind::Snapshot)
            } else if ident_str.eq(ident_str!("DerivedStringSnapshot")) {
                Some(IdentifierMappingKind::DerivedString)
            } else {
                None
            }
        };

        (struct_name.module.address().eq(&AccountAddress::ONE)
            && struct_name.module.name().eq(ident_str!("aggregator_v2")))
        .then_some(ident_str_to_kind(struct_name.name.as_ident_str()))
        .flatten()
    }

    fn check_depth_and_increment_count(&self, count: &mut u64, depth: u64) -> PartialVMResult<()> {
        let max_nodes = self
            .runtime_environment()
            .vm_config()
            .layout_max_size
            .max(MAX_TYPE_TO_LAYOUT_NODES);
        if *count > max_nodes {
            return Err(
                PartialVMError::new(StatusCode::TOO_MANY_TYPE_NODES).with_message(format!(
                    "Number of type nodes when constructing type layout exceeded the maximum of {}",
                    max_nodes
                )),
            );
        }
        if depth > self
            .runtime_environment()
            .vm_config()
            .layout_max_depth
            .max(VALUE_DEPTH_MAX)
        {
            return Err(
                PartialVMError::new(StatusCode::VM_MAX_VALUE_DEPTH_REACHED).with_message(format!(
                    "Depth of a layout exceeded the maximum of {} during construction",
                    self.runtime_environment().vm_config().layout_max_depth.max(VALUE_DEPTH_MAX)
                )),
            );
        }
        Ok(())
    }

    fn type_to_type_layout_impl(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
        count: &mut u64,
        depth: u64,
    ) -> PartialVMResult<(MoveTypeLayout, bool)> {
        self.check_depth_and_increment_count(count, depth)?;
        Ok(match ty {
            Type::Bool => {
                *count += 1;
                (MoveTypeLayout::Bool, false)
            },
            Type::U8 => {
                *count += 1;
                (MoveTypeLayout::U8, false)
            },
            Type::U16 => {
                *count += 1;
                (MoveTypeLayout::U16, false)
            },
            Type::U32 => {
                *count += 1;
                (MoveTypeLayout::U32, false)
            },
            Type::U64 => {
                *count += 1;
                (MoveTypeLayout::U64, false)
            },
            Type::U128 => {
                *count += 1;
                (MoveTypeLayout::U128, false)
            },
            Type::U256 => {
                *count += 1;
                (MoveTypeLayout::U256, false)
            },
            Type::Address => {
                *count += 1;
                (MoveTypeLayout::Address, false)
            },
            Type::Signer => {
                *count += 1;
                (MoveTypeLayout::Signer, false)
            },
            Type::Vector(ty) => {
                *count += 1;
                let (layout, has_identifier_mappings) =
                    self.type_to_type_layout_impl(gas_meter, traversal_context, ty, count, depth + 1)?;
                (MoveTypeLayout::Vector(Box::new(layout)), has_identifier_mappings)
            },
            Type::Struct { idx, .. } => {
                *count += 1;
                self.struct_name_to_type_layout(gas_meter, traversal_context, idx, &[], count, depth + 1)?
            },
            Type::StructInstantiation { idx, ty_args, .. } => {
                *count += 1;
                self.struct_name_to_type_layout(
                    gas_meter,
                    traversal_context,
                    idx,
                    ty_args,
                    count,
                    depth + 1,
                )?
            },
            Type::Reference(_) | Type::MutableReference(_) | Type::TyParam(_) => {
                return Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message(format!("No type layout for {:?}", ty)),
                );
            },
        })
    }

    fn struct_name_to_type_layout(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        struct_idx: &StructNameIndex,
        ty_args: &[Type],
        count: &mut u64,
        depth: u64,
    ) -> PartialVMResult<(MoveTypeLayout, bool)> {
        let cache_key = StructLayoutKey {
            idx: *struct_idx,
            ty_args: ty_args.to_vec(),
        };
        if let Some((layout, has_identifier_mappings)) =
            self.layout_cache.borrow().get(&cache_key).cloned()
        {
            return Ok((layout, has_identifier_mappings));
        }

        let struct_name = self
            .runtime_environment()
            .struct_name_index_map()
            .idx_to_struct_name_ref(*struct_idx)?;
        let struct_type = self
            .struct_definition_loader
            .load_struct_definition(gas_meter, traversal_context, struct_idx)?;
        let maybe_mapping = self.get_identifier_mapping_kind(struct_name.as_ref());
        let field_tys = struct_type
            .field_tys
            .iter()
            .map(|ty| {
                self.runtime_environment()
                    .vm_config()
                    .ty_builder
                    .create_ty_with_subst_with_legacy_check(ty, ty_args)
            })
            .collect::<PartialVMResult<Vec<_>>>()?;
        let (mut field_layouts, field_has_identifier_mappings): (Vec<_>, Vec<_>) = field_tys
            .iter()
            .map(|ty| self.type_to_type_layout_impl(gas_meter, traversal_context, ty, count, depth))
            .collect::<PartialVMResult<Vec<_>>>()?
            .into_iter()
            .unzip();

        let has_identifier_mappings =
            maybe_mapping.is_some() || field_has_identifier_mappings.into_iter().any(|b| b);
        let layout = if Some(IdentifierMappingKind::DerivedString) == maybe_mapping {
            MoveTypeLayout::Native(
                IdentifierMappingKind::DerivedString,
                Box::new(MoveTypeLayout::Struct(MoveStructLayout::new(field_layouts))),
            )
        } else {
            if let Some(kind) = &maybe_mapping {
                if let Some(first) = field_layouts.first_mut() {
                    *first = MoveTypeLayout::Native(kind.clone(), Box::new(first.clone()));
                }
            }
            MoveTypeLayout::Struct(MoveStructLayout::new(field_layouts))
        };
        self.layout_cache.borrow_mut().insert(
            StructLayoutKey {
                idx: *struct_idx,
                ty_args: ty_args.to_vec(),
            },
            (layout.clone(), has_identifier_mappings),
        );
        Ok((layout, has_identifier_mappings))
    }

    fn type_to_fully_annotated_layout_impl(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
        count: &mut u64,
        depth: u64,
    ) -> PartialVMResult<MoveTypeLayout> {
        self.check_depth_and_increment_count(count, depth)?;
        Ok(match ty {
            Type::Bool => MoveTypeLayout::Bool,
            Type::U8 => MoveTypeLayout::U8,
            Type::U16 => MoveTypeLayout::U16,
            Type::U32 => MoveTypeLayout::U32,
            Type::U64 => MoveTypeLayout::U64,
            Type::U128 => MoveTypeLayout::U128,
            Type::U256 => MoveTypeLayout::U256,
            Type::Address => MoveTypeLayout::Address,
            Type::Signer => MoveTypeLayout::Signer,
            Type::Vector(ty) => MoveTypeLayout::Vector(Box::new(
                self.type_to_fully_annotated_layout_impl(
                    gas_meter,
                    traversal_context,
                    ty,
                    count,
                    depth + 1,
                )?,
            )),
            Type::Struct { idx, .. } => {
                self.struct_name_to_fully_annotated_layout(gas_meter, traversal_context, idx, &[], count, depth + 1)?
            },
            Type::StructInstantiation { idx, ty_args, .. } => {
                self.struct_name_to_fully_annotated_layout(
                    gas_meter,
                    traversal_context,
                    idx,
                    ty_args,
                    count,
                    depth + 1,
                )?
            },
            Type::Reference(_) | Type::MutableReference(_) | Type::TyParam(_) => {
                return Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message(format!("No type layout for {:?}", ty)),
                );
            },
        })
    }

    fn struct_name_to_fully_annotated_layout(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        struct_idx: &StructNameIndex,
        ty_args: &[Type],
        count: &mut u64,
        depth: u64,
    ) -> PartialVMResult<MoveTypeLayout> {
        let cache_key = StructLayoutKey {
            idx: *struct_idx,
            ty_args: ty_args.to_vec(),
        };
        if let Some(layout) = self.annotated_layout_cache.borrow().get(&cache_key).cloned() {
            return Ok(layout);
        }

        let struct_name = self
            .runtime_environment()
            .struct_name_index_map()
            .idx_to_struct_name_ref(*struct_idx)?;
        let struct_type = self
            .struct_definition_loader
            .load_struct_definition(gas_meter, traversal_context, struct_idx)?;
        if struct_type.field_tys.len() != struct_type.field_names.len() {
            return Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR).with_message(
                    format!(
                        "Field types did not match field names in {}::{}",
                        struct_name.module,
                        struct_name.name
                    ),
                ),
            );
        }

        let struct_tag = TypeTagConverter::new(self.runtime_environment())
            .struct_name_idx_to_struct_tag(struct_idx, ty_args)?;
        let field_layouts = struct_type
            .field_names
            .iter()
            .zip(struct_type.field_tys.iter())
            .map(|(field_name, ty)| {
                let ty = self
                    .runtime_environment()
                    .vm_config()
                    .ty_builder
                    .create_ty_with_subst_with_legacy_check(ty, ty_args)?;
                let layout = self.type_to_fully_annotated_layout_impl(
                    gas_meter,
                    traversal_context,
                    &ty,
                    count,
                    depth,
                )?;
                Ok(MoveFieldLayout::new(field_name.clone(), layout))
            })
            .collect::<PartialVMResult<Vec<_>>>()?;
        let layout = MoveTypeLayout::Struct(MoveStructLayout::with_types(struct_tag, field_layouts));
        self.annotated_layout_cache.borrow_mut().insert(
            StructLayoutKey {
                idx: *struct_idx,
                ty_args: ty_args.to_vec(),
            },
            layout.clone(),
        );
        Ok(layout)
    }

    pub(crate) fn type_to_type_layout_unmetered(
        &self,
        ty: &Type,
    ) -> PartialVMResult<(MoveTypeLayout, bool)> {
        let mut gas_meter = UnmeteredGasMeter;
        let storage = crate::module_traversal::TraversalStorage::new();
        let mut traversal_context = TraversalContext::new(&storage);
        self.type_to_type_layout_with_identifier_mappings(&mut gas_meter, &mut traversal_context, ty)
    }
}
