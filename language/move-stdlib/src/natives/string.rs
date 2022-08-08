// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Implementation of native functions for utf8 strings.

use move_binary_format::errors::PartialVMResult;
use move_vm_runtime::native_functions::NativeContext;
use move_core_types::vm_status::sub_status::NFE_STRING_INVALID_ARG_FAILURE;
use move_vm_types::{
    loaded_data::runtime_types::Type,
    natives::function::{native_gas, NativeResult},
    pop_arg,
    values::Value,
    gas_schedule::NativeCostIndex,
};
use std::collections::VecDeque;
// The implementation approach delegates all utf8 handling to Rust.
// This is possible without copying of bytes because (a) we can
// get a `std::cell::Ref<Vec<u8>>` from a `vector<u8>` and in turn a `&[u8]`
// from that (b) assuming that `vector<u8>` embedded in a string
// is already valid utf8, we can use `str::from_utf8_unchecked` to
// create a `&str` view on the bytes without a copy. Once we have this
// view, we can call ut8 functions like length, substring, etc.

/***************************************************************************************************
 * native fun native_check_utf8
 *
 *   
 *
 **************************************************************************************************/

pub fn native_check_utf8(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(arguments.len() == 1);
    let s_arg = pop_arg!(arguments, Vec<u8>);
    let ok = std::str::from_utf8(s_arg.as_slice()).is_ok();

    let cost = native_gas(
        context.cost_table(),
        NativeCostIndex::STRING_CHECK_UT8 as u8,
        0,
    );
    NativeResult::map_partial_vm_result_one(cost, Ok(Value::bool(ok)))
}

/***************************************************************************************************
 * native fun native_is_char_boundary
 *
 *  
 *
 **************************************************************************************************/
pub fn native_is_char_boundary(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(arguments.len() == 2);
    let i = pop_arg!(arguments, u64);
    let s_arg = pop_arg!(arguments, Vec<u8>);
    let ok = unsafe {
        // This is safe because we guarantee the bytes to be utf8.
        std::str::from_utf8_unchecked(s_arg.as_slice()).is_char_boundary(i as usize)
    };
    let cost = native_gas(
        context.cost_table(),
        NativeCostIndex::SRING_CHAR_BOUNDARY as u8,
        0,
    );
    NativeResult::map_partial_vm_result_one(cost, Ok(Value::bool(ok)))
}

/***************************************************************************************************
 * native fun native_sub_string
 *
 *  
 *
 **************************************************************************************************/

pub fn native_sub_string(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(arguments.len() == 3);
    let j = pop_arg!(arguments, u64) as usize;
    let i = pop_arg!(arguments, u64) as usize;
    let cost = native_gas(
        context.cost_table(),
        NativeCostIndex::STRING_SUB_STR as u8,
        0,
    );
    if j < i {
        // TODO: what abort code should we use here?
        return Ok(NativeResult::err(cost, NFE_STRING_INVALID_ARG_FAILURE));
    }

    let s_arg = pop_arg!(arguments, Vec<u8>);
    let s_str = unsafe {
        // This is safe because we guarantee the bytes to be utf8.
        std::str::from_utf8_unchecked(s_arg.as_slice())
    };
    let v = Value::vector_u8((&s_str[i..j]).as_bytes().iter().cloned());

    
    NativeResult::map_partial_vm_result_one(cost, Ok(v))
}

/***************************************************************************************************
 * native fun native_index_of
 *
 *  
 *
 **************************************************************************************************/

pub fn native_index_of(
    context: &mut NativeContext,
    ty_args: Vec<Type>,
    mut arguments: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    debug_assert!(ty_args.is_empty());
    debug_assert!(arguments.len() == 2);
    let r_arg = pop_arg!(arguments, Vec<u8>);
    let r_str = unsafe { std::str::from_utf8_unchecked(r_arg.as_slice()) };
    let s_arg = pop_arg!(arguments, Vec<u8>);
    let s_str = unsafe { std::str::from_utf8_unchecked(s_arg.as_slice()) };
    let pos = match s_str.find(r_str) {
        Some(size) => size,
        None => s_str.len(),
    };
    let cost = native_gas(
        context.cost_table(),
        NativeCostIndex::STRING_INDEX_OF as u8,
        0,
    );
    NativeResult::map_partial_vm_result_one(cost, Ok(Value::u64(pos as u64)))
}
