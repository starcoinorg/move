// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{module_traversal::TraversalContext, ModuleStorage};
use move_binary_format::{
    access::ModuleAccess,
    errors::{Location, VMResult},
};
use move_core_types::{
    account_address::AccountAddress,
    gas_algebra::NumBytes,
    identifier::IdentStr,
    language_storage::{ModuleId, StructTag, TypeTag},
};
use move_vm_types::gas::GasMeter;
use std::collections::BTreeSet;

fn collect_struct_tag_deps(ordered_ty_tags: &mut BTreeSet<ModuleId>, ty_tag: &TypeTag) {
    match ty_tag {
        TypeTag::Vector(inner) => collect_struct_tag_deps(ordered_ty_tags, inner),
        TypeTag::Struct(struct_tag) => {
            collect_struct_tag_deps_from_struct(ordered_ty_tags, struct_tag)
        }
        TypeTag::Bool
        | TypeTag::U8
        | TypeTag::U16
        | TypeTag::U32
        | TypeTag::U64
        | TypeTag::U128
        | TypeTag::U256
        | TypeTag::Address
        | TypeTag::Signer => {}
    }
}

fn collect_struct_tag_deps_from_struct(
    ordered_ty_tags: &mut BTreeSet<ModuleId>,
    struct_tag: &StructTag,
) {
    ordered_ty_tags.insert(ModuleId::new(struct_tag.address, struct_tag.module.clone()));
    for ty_arg in &struct_tag.type_args {
        collect_struct_tag_deps(ordered_ty_tags, ty_arg);
    }
}

pub fn check_type_tag_dependencies_and_charge_gas<'a>(
    module_storage: &impl ModuleStorage,
    gas_meter: &mut impl GasMeter,
    traversal_context: &mut TraversalContext<'a>,
    ty_tags: &[TypeTag],
) -> VMResult<()> {
    let ordered_ty_tags = ty_tags
        .iter()
        .fold(BTreeSet::new(), |mut ordered_ty_tags, ty_tag| {
            collect_struct_tag_deps(&mut ordered_ty_tags, ty_tag);
            ordered_ty_tags
        });
    let ordered_ty_tags = ordered_ty_tags
        .into_iter()
        .map(|module_id| traversal_context.referenced_module_ids.alloc(module_id))
        .collect::<Vec<_>>();

    check_dependencies_and_charge_gas(
        module_storage,
        gas_meter,
        traversal_context,
        ordered_ty_tags
            .into_iter()
            .map(|module_id| (module_id.address(), module_id.name())),
    )
}

pub fn check_dependencies_and_charge_gas<'a, I>(
    module_storage: &impl ModuleStorage,
    gas_meter: &mut impl GasMeter,
    traversal_context: &mut TraversalContext<'a>,
    ids: I,
) -> VMResult<()>
where
    I: IntoIterator<Item = (&'a AccountAddress, &'a IdentStr)>,
    I::IntoIter: DoubleEndedIterator,
{
    let mut stack = Vec::with_capacity(512);
    traversal_context.push_next_ids_to_visit(&mut stack, ids);

    while let Some((addr, name)) = stack.pop() {
        let size = module_storage.unmetered_get_existing_module_size(addr, name)?;
        gas_meter
            .charge_dependency(false, addr, name, NumBytes::new(size as u64))
            .map_err(|err| err.finish(Location::Module(ModuleId::new(*addr, name.to_owned()))))?;

        let compiled_module =
            module_storage.unmetered_get_existing_deserialized_module(addr, name)?;
        let compiled_module = traversal_context.referenced_modules.alloc(compiled_module);
        let imm_deps_and_friends = compiled_module
            .immediate_dependencies_iter()
            .chain(compiled_module.immediate_friends_iter());
        traversal_context.push_next_ids_to_visit(&mut stack, imm_deps_and_friends);
    }

    Ok(())
}
