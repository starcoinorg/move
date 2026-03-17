// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{config::VMConfig, RuntimeEnvironment};
use hashbrown::HashMap;
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::{
    language_storage::{StructTag, TypeTag},
    vm_status::StatusCode,
};
use move_vm_types::loaded_data::runtime_types::{StructNameIndex, Type};
use parking_lot::RwLock;
use std::hash::{Hash, Hasher};

struct PseudoGasContext {
    max_cost: u64,
    cost: u64,
    cost_base: u64,
    cost_per_byte: u64,
}

impl PseudoGasContext {
    fn new(vm_config: &VMConfig) -> Self {
        Self {
            max_cost: vm_config.type_max_cost,
            cost: 0,
            cost_base: vm_config.type_base_cost,
            cost_per_byte: vm_config.type_byte_cost,
        }
    }

    fn current_cost(&self) -> u64 {
        self.cost
    }

    fn charge_base(&mut self) -> PartialVMResult<()> {
        self.charge(self.cost_base)
    }

    fn charge_struct_tag(&mut self, struct_tag: &StructTag) -> PartialVMResult<()> {
        let size =
            (struct_tag.address.len() + struct_tag.module.len() + struct_tag.name.len()) as u64;
        self.charge(size * self.cost_per_byte)
    }

    fn charge(&mut self, amount: u64) -> PartialVMResult<()> {
        self.cost += amount;
        if self.cost > self.max_cost {
            Err(
                PartialVMError::new(StatusCode::TYPE_TAG_LIMIT_EXCEEDED).with_message(format!(
                    "Exceeded maximum type tag limit of {} when charging {}",
                    self.max_cost, amount
                )),
            )
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct StructKey {
    idx: StructNameIndex,
    ty_args: Vec<Type>,
}

#[derive(Eq, PartialEq)]
struct StructKeyRef<'a> {
    idx: &'a StructNameIndex,
    ty_args: &'a [Type],
}

impl<'a> hashbrown::Equivalent<StructKeyRef<'a>> for StructKey {
    fn equivalent(&self, other: &StructKeyRef<'a>) -> bool {
        &self.idx == other.idx && self.ty_args.as_slice() == other.ty_args
    }
}

impl hashbrown::Equivalent<StructKey> for StructKeyRef<'_> {
    fn equivalent(&self, other: &StructKey) -> bool {
        self.idx == &other.idx && self.ty_args == other.ty_args.as_slice()
    }
}

impl Hash for StructKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
        self.ty_args.hash(state);
    }
}

impl Hash for StructKeyRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.idx.hash(state);
        self.ty_args.hash(state);
    }
}

#[derive(Debug, Clone)]
struct PricedStructTag {
    struct_tag: StructTag,
    pseudo_gas_cost: u64,
}

pub struct TypeTagCache {
    cache: RwLock<HashMap<StructKey, PricedStructTag>>,
}

impl TypeTagCache {
    pub(crate) fn empty() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    fn get_struct_tag(&self, idx: &StructNameIndex, ty_args: &[Type]) -> Option<PricedStructTag> {
        self.cache
            .read()
            .get(&StructKeyRef { idx, ty_args })
            .cloned()
    }

    fn insert_struct_tag(
        &self,
        idx: &StructNameIndex,
        ty_args: &[Type],
        priced_struct_tag: PricedStructTag,
    ) {
        let mut cache = self.cache.write();
        cache
            .entry(StructKey {
                idx: *idx,
                ty_args: ty_args.to_vec(),
            })
            .or_insert(priced_struct_tag);
    }
}

pub struct TypeTagConverter<'a> {
    runtime_environment: &'a RuntimeEnvironment,
    cache: TypeTagCache,
}

impl<'a> TypeTagConverter<'a> {
    pub(crate) fn new(runtime_environment: &'a RuntimeEnvironment) -> Self {
        Self {
            runtime_environment,
            cache: TypeTagCache::empty(),
        }
    }

    pub(crate) fn ty_to_ty_tag(&self, ty: &Type) -> PartialVMResult<TypeTag> {
        let mut gas_context = PseudoGasContext::new(self.runtime_environment.vm_config());
        self.ty_to_ty_tag_impl(ty, &mut gas_context)
    }

    pub(crate) fn struct_name_idx_to_struct_tag(
        &self,
        struct_name_idx: &StructNameIndex,
        ty_args: &[Type],
    ) -> PartialVMResult<StructTag> {
        let mut gas_context = PseudoGasContext::new(self.runtime_environment.vm_config());
        self.struct_name_idx_to_struct_tag_impl(struct_name_idx, ty_args, &mut gas_context)
    }

    fn ty_to_ty_tag_impl(
        &self,
        ty: &Type,
        gas_context: &mut PseudoGasContext,
    ) -> PartialVMResult<TypeTag> {
        gas_context.charge_base()?;
        Ok(match ty {
            Type::Bool => TypeTag::Bool,
            Type::U8 => TypeTag::U8,
            Type::U16 => TypeTag::U16,
            Type::U32 => TypeTag::U32,
            Type::U64 => TypeTag::U64,
            Type::U128 => TypeTag::U128,
            Type::U256 => TypeTag::U256,
            Type::Address => TypeTag::Address,
            Type::Signer => TypeTag::Signer,
            Type::Vector(ty) => TypeTag::Vector(Box::new(self.ty_to_ty_tag_impl(ty, gas_context)?)),
            Type::Struct { idx, .. } => TypeTag::Struct(Box::new(
                self.struct_name_idx_to_struct_tag_impl(idx, &[], gas_context)?,
            )),
            Type::StructInstantiation { idx, ty_args, .. } => TypeTag::Struct(Box::new(
                self.struct_name_idx_to_struct_tag_impl(idx, ty_args, gas_context)?,
            )),
            Type::Reference(_) | Type::MutableReference(_) | Type::TyParam(_) => {
                return Err(
                    PartialVMError::new(StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR)
                        .with_message(format!("No type tag for {:?}", ty)),
                );
            }
        })
    }

    fn struct_name_idx_to_struct_tag_impl(
        &self,
        struct_name_idx: &StructNameIndex,
        ty_args: &[Type],
        gas_context: &mut PseudoGasContext,
    ) -> PartialVMResult<StructTag> {
        if let Some(priced_struct_tag) = self.cache.get_struct_tag(struct_name_idx, ty_args) {
            gas_context.charge(priced_struct_tag.pseudo_gas_cost)?;
            return Ok(priced_struct_tag.struct_tag);
        }

        let struct_name = self
            .runtime_environment
            .struct_name_index_map()
            .idx_to_struct_name_ref(*struct_name_idx)?;
        let initial_cost = gas_context.current_cost();
        let type_args = ty_args
            .iter()
            .map(|ty| self.ty_to_ty_tag_impl(ty, gas_context))
            .collect::<PartialVMResult<Vec<_>>>()?;
        let struct_tag = StructTag {
            address: *struct_name.module.address(),
            module: struct_name.module.name().to_owned(),
            name: struct_name.name.clone(),
            type_args,
        };
        gas_context.charge_struct_tag(&struct_tag)?;
        self.cache.insert_struct_tag(
            struct_name_idx,
            ty_args,
            PricedStructTag {
                struct_tag: struct_tag.clone(),
                pseudo_gas_cost: gas_context.current_cost() - initial_cost,
            },
        );
        Ok(struct_tag)
    }
}
