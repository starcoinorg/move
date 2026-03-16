// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! The core Move VM logic.
//!
//! It is a design goal for the Move VM to be independent of the Diem blockchain, so that
//! other blockchains can use it as well. The VM isn't there yet, but hopefully will be there
//! soon.

pub mod data_cache;
mod interpreter;
mod loader;
pub mod logging;
pub mod move_vm;
pub mod native_extensions;
pub mod native_functions;
mod runtime;
pub mod session;
#[macro_use]
pub mod tracing;
pub mod config;
pub mod move_vm_adapter;
pub mod module_traversal;
mod storage;

// Only include debugging functionality in debug builds
#[cfg(any(debug_assertions, feature = "debugging"))]
mod debug;

mod access_control;

pub use loader::LoadedFunction;
pub use storage::{
    code_storage::CodeStorage,
    environment::{
        ambassador_impl_WithRuntimeEnvironment, RuntimeEnvironment, WithRuntimeEnvironment,
    },
    implementations::{
        unsync_code_storage::{AsUnsyncCodeStorage, UnsyncCodeStorage},
        unsync_module_storage::{AsUnsyncModuleStorage, BorrowedOrOwned, UnsyncModuleStorage},
    },
    layout_cache::{LayoutCache, LayoutCacheEntry, NoOpLayoutCache, StructKey},
    module_storage::{
        ambassador_impl_ModuleStorage, AsFunctionValueExtension, FunctionValueExtensionAdapter,
        ModuleStorage,
    },
};
