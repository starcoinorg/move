// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

//! Core types for Move.

#![feature(error_in_core)]
#![cfg_attr(feature = "nostd", no_std)]

#[cfg(feature = "nostd")]
extern crate alloc;

pub mod abi;
pub mod account_address;
pub mod effects;
pub mod errmap;
pub mod gas_schedule;
pub mod identifier;
pub mod language_storage;
pub mod metadata;
pub mod move_resource;
pub mod parser;
#[cfg(any(test, feature = "fuzzing"))]
pub mod proptest_types;
pub mod resolver;
pub mod transaction_argument;
#[cfg(not(feature = "nostd"))]
#[cfg(test)]
mod unit_tests;
pub mod value;
pub mod vm_status;
