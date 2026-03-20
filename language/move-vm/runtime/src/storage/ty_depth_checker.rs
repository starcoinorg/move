// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{module_traversal::TraversalContext, storage::loader::traits::StructDefinitionLoader};
use move_binary_format::{
    errors::{PartialVMError, PartialVMResult},
    file_format::TypeParameterIndex,
};
use move_core_types::vm_status::StatusCode;
use move_vm_types::{
    gas::GasMeter,
    loaded_data::runtime_types::{DepthFormula, StructNameIndex, Type},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};

pub struct TypeDepthChecker<'a, T> {
    struct_definition_loader: &'a T,
    maybe_max_depth: Option<u64>,
    formula_cache: RefCell<HashMap<StructNameIndex, DepthFormula>>,
}

impl<'a, T> TypeDepthChecker<'a, T>
where
    T: StructDefinitionLoader,
{
    pub(crate) fn new(struct_definition_loader: &'a T) -> Self {
        let vm_config = struct_definition_loader.runtime_environment().vm_config();
        let maybe_max_depth = vm_config
            .enable_depth_checks
            .then_some(vm_config.max_value_nest_depth)
            .flatten();
        Self {
            struct_definition_loader,
            maybe_max_depth,
            formula_cache: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn check_depth_of_type(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
    ) -> PartialVMResult<()> {
        let max_depth = match self.maybe_max_depth {
            Some(max_depth) => max_depth,
            None => return Ok(()),
        };
        self.recursive_check_depth_of_type(gas_meter, traversal_context, ty, max_depth, 1)?;
        Ok(())
    }

    fn recursive_check_depth_of_type(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        ty: &Type,
        max_depth: u64,
        depth: u64,
    ) -> PartialVMResult<u64> {
        macro_rules! check_depth {
            ($additional_depth:expr) => {{
                let new_depth = depth.saturating_add($additional_depth);
                if new_depth > max_depth {
                    return Err(PartialVMError::new(StatusCode::VM_MAX_VALUE_DEPTH_REACHED));
                }
                new_depth
            }};
        }

        macro_rules! visit_struct {
            ($idx:expr) => {{
                let mut currently_visiting = HashSet::new();
                let formula = self.calculate_struct_depth_formula(
                    gas_meter,
                    traversal_context,
                    &mut currently_visiting,
                    &$idx,
                )?;
                if !currently_visiting.is_empty() {
                    let struct_name = self
                        .struct_definition_loader
                        .runtime_environment()
                        .struct_name_index_map()
                        .idx_to_struct_name_ref($idx)?;
                    return Err(
                        PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                            .with_message(format!(
                                "Constructing a formula for {}::{}::{} has non-empty visiting set",
                                struct_name.module.address(),
                                struct_name.module.name(),
                                struct_name.name
                            )),
                    );
                }
                formula
            }};
        }

        let ty_depth = match ty {
            Type::Bool
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::U128
            | Type::U256
            | Type::Address
            | Type::Signer => check_depth!(0),
            Type::Reference(ty) | Type::MutableReference(ty) => self
                .recursive_check_depth_of_type(
                    gas_meter,
                    traversal_context,
                    ty,
                    max_depth,
                    check_depth!(1),
                )?,
            Type::Vector(ty) => self.recursive_check_depth_of_type(
                gas_meter,
                traversal_context,
                ty,
                max_depth,
                check_depth!(1),
            )?,
            Type::Struct { idx, .. } => {
                let formula = visit_struct!(*idx);
                check_depth!(formula.solve(&[]))
            }
            Type::StructInstantiation { idx, ty_args, .. } => {
                let ty_arg_depths = ty_args
                    .iter()
                    .map(|ty| {
                        self.recursive_check_depth_of_type(
                            gas_meter,
                            traversal_context,
                            ty,
                            max_depth,
                            check_depth!(0),
                        )
                    })
                    .collect::<PartialVMResult<Vec<_>>>()?;
                let formula = visit_struct!(*idx);
                check_depth!(formula.solve(&ty_arg_depths))
            }
            Type::TyParam(_) => {
                return Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message("Type parameter should be fully resolved".to_string()),
                );
            }
        };

        Ok(ty_depth)
    }

    fn calculate_struct_depth_formula(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        currently_visiting: &mut HashSet<StructNameIndex>,
        idx: &StructNameIndex,
    ) -> PartialVMResult<DepthFormula> {
        if currently_visiting.contains(idx) {
            let struct_name = self
                .struct_definition_loader
                .runtime_environment()
                .struct_name_index_map()
                .idx_to_struct_name_ref(*idx)?;
            return Err(
                PartialVMError::new(StatusCode::CYCLIC_MODULE_DEPENDENCY).with_message(format!(
                    "Definition of struct {}::{}::{} is recursive",
                    struct_name.module.address(),
                    struct_name.module.name(),
                    struct_name.name
                )),
            );
        }

        if let Some(formula) = self.formula_cache.borrow().get(idx) {
            return Ok(formula.clone());
        }

        assert!(currently_visiting.insert(*idx));
        let struct_definition = self.struct_definition_loader.load_struct_definition(
            gas_meter,
            traversal_context,
            idx,
        )?;
        let formulas = struct_definition
            .field_tys
            .iter()
            .map(|field_type| {
                self.calculate_type_depth_formula(
                    gas_meter,
                    traversal_context,
                    currently_visiting,
                    field_type,
                )
            })
            .collect::<PartialVMResult<Vec<_>>>()?;
        let formula = DepthFormula::normalize(formulas);

        assert!(currently_visiting.remove(idx));
        let prev = self
            .formula_cache
            .borrow_mut()
            .insert(*idx, formula.clone());
        if prev.is_some() {
            self.formula_cache.borrow_mut().clear();
            return Err(
                PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR).with_message(
                    format!("Depth formula for struct {:?} is already cached", idx),
                ),
            );
        }
        Ok(formula)
    }

    fn calculate_type_depth_formula(
        &self,
        gas_meter: &mut impl GasMeter,
        traversal_context: &mut TraversalContext,
        currently_visiting: &mut HashSet<StructNameIndex>,
        ty: &Type,
    ) -> PartialVMResult<DepthFormula> {
        Ok(match ty {
            Type::Bool
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::U128
            | Type::U256
            | Type::Address
            | Type::Signer => DepthFormula::constant(1),
            Type::Vector(ty) => {
                let mut inner = self.calculate_type_depth_formula(
                    gas_meter,
                    traversal_context,
                    currently_visiting,
                    ty,
                )?;
                inner.scale(1);
                inner
            }
            Type::Reference(ty) | Type::MutableReference(ty) => {
                let mut inner = self.calculate_type_depth_formula(
                    gas_meter,
                    traversal_context,
                    currently_visiting,
                    ty,
                )?;
                inner.scale(1);
                inner
            }
            Type::TyParam(ty_idx) => DepthFormula::type_parameter(*ty_idx),
            Type::Struct { idx, .. } => {
                let mut struct_formula = self.calculate_struct_depth_formula(
                    gas_meter,
                    traversal_context,
                    currently_visiting,
                    idx,
                )?;
                debug_assert!(struct_formula.terms.is_empty());
                struct_formula.scale(1);
                struct_formula
            }
            Type::StructInstantiation { idx, ty_args, .. } => {
                let ty_arg_map = ty_args
                    .iter()
                    .enumerate()
                    .map(|(idx, ty)| {
                        let var = idx as TypeParameterIndex;
                        Ok((
                            var,
                            self.calculate_type_depth_formula(
                                gas_meter,
                                traversal_context,
                                currently_visiting,
                                ty,
                            )?,
                        ))
                    })
                    .collect::<PartialVMResult<BTreeMap<_, _>>>()?;
                let struct_formula = self.calculate_struct_depth_formula(
                    gas_meter,
                    traversal_context,
                    currently_visiting,
                    idx,
                )?;
                let mut subst_struct_formula = struct_formula.subst(ty_arg_map)?;
                subst_struct_formula.scale(1);
                subst_struct_formula
            }
        })
    }
}
