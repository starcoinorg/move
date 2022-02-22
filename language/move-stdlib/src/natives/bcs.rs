// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::errors::PartialVMResult;
use move_core_types::vm_status::sub_status::NFE_BCS_SERIALIZATION_FAILURE;
use move_vm_runtime::native_functions::NativeContext;
use move_vm_types::{
    gas_schedule::NativeCostIndex,
    loaded_data::runtime_types::Type,
    natives::function::{native_gas, NativeResult},
    pop_arg,
    values::{values_impl::Reference, Value},
};
use smallvec::smallvec;
use std::collections::VecDeque;
// use std::convert::TryFrom;

/// Rust implementation of Move's `native public fun to_bytes<T>(&T): vector<u8>`
pub fn native_to_bytes(
    context: &mut NativeContext,
    mut ty_args: Vec<Type>,
    mut args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.len() == 1);
    debug_assert!(args.len() == 1);

    let ref_to_val = pop_arg!(args, Reference);

    let arg_type = ty_args.pop().unwrap();
    // delegate to the BCS serialization for `Value`
    let serialized_value_opt = match context.type_to_type_layout(&arg_type)? {
        None => None,
        Some(layout) => ref_to_val.read_ref()?.simple_serialize(&layout),
    };
    let serialized_value = match serialized_value_opt {
        None => {
            let cost = native_gas(context.cost_table(), NativeCostIndex::BCS_TO_BYTES as u8, 1);
            return Ok(NativeResult::err(cost, NFE_BCS_SERIALIZATION_FAILURE));
        }
        Some(serialized_value) => serialized_value,
    };

    // cost is proportional to the size of the serialized value
    let cost = native_gas(
        context.cost_table(),
        NativeCostIndex::BCS_TO_BYTES as u8,
        serialized_value.len(),
    );

    Ok(NativeResult::ok(
        cost,
        smallvec![Value::vector_u8(serialized_value)],
    ))
}

// /// Rust implementation of Move's `public fun from_public_key_vec(pub_key_vec: vector<u8>): address;`
// pub fn native_to_address(
//     context: &mut NativeContext,
//     mut _ty_args: Vec<Type>,
//     mut args: VecDeque<Value>,
// ) -> PartialVMResult<NativeResult> {
//     debug_assert!(_ty_args.is_empty());
//     debug_assert!(args.len() == 1);
//
//     let key_bytes = pop_arg!(args, Vec<u8>);
//     let cost = native_gas(
//         context.cost_table(),
//         NativeCostIndex::BCS_TO_ADDRESS,
//         key_bytes.len(),
//     );
//     if key_bytes.len() != AccountAddress::LENGTH {
//         return Ok(NativeResult::err(cost, NFE_BCS_TO_ADDRESS_FAILURE));
//     }
//
//     let address = match AccountAddress::try_from(&key_bytes[..AccountAddress::LENGTH]) {
//         Ok(addr) => addr,
//         Err(_) => return Ok(NativeResult::err(cost, NFE_BCS_TO_ADDRESS_FAILURE)),
//     };
//     let return_values = smallvec![Value::address(address)];
//     Ok(NativeResult::ok(cost, return_values))
// }
