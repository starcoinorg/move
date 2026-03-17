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
mod execution_context;
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
pub mod module_traversal;
pub mod move_vm_adapter;
mod storage;

// Only include debugging functionality in debug builds
#[cfg(any(debug_assertions, feature = "debugging"))]
mod debug;

mod access_control;

pub use loader::LoadedFunction;
pub use storage::{
    code_storage::CodeStorage,
    dependencies_gas_charging::check_dependencies_and_charge_gas,
    environment::{
        ambassador_impl_WithRuntimeEnvironment, RuntimeEnvironment, RuntimeEnvironmentRef,
        WithRuntimeEnvironment,
    },
    implementations::{
        unsync_code_storage::{AsUnsyncCodeStorage, UnsyncCodeStorage},
        unsync_module_storage::{AsUnsyncModuleStorage, BorrowedOrOwned, UnsyncModuleStorage},
    },
    layout_cache::{LayoutCache, LayoutCacheEntry, NoOpLayoutCache, StructKey},
    loader::{
        eager::EagerLoader,
        lazy::LazyLoader,
        traits::{
            FunctionDefinitionLoader, InstantiatedFunctionLoader, LegacyLoaderConfig,
            Loader as StorageLoader, ModuleMetadataLoader, NativeModuleLoader, ScriptLoader,
            StructDefinitionLoader,
        },
    },
    module_storage::{
        ambassador_impl_ModuleStorage, AsFunctionValueExtension, FunctionValueExtensionAdapter,
        ModuleStorage,
    },
    publishing::{StagingModuleStorage, VerifiedModuleBundle},
};

#[macro_export]
macro_rules! dispatch_loader {
    ($module_storage:expr, $loader:ident, $dispatch:expr) => {
        if $crate::WithRuntimeEnvironment::runtime_environment($module_storage)
            .vm_config()
            .enable_lazy_loading
        {
            let $loader = $crate::LazyLoader::new($module_storage);
            $dispatch
        } else {
            let $loader = $crate::EagerLoader::new($module_storage);
            $dispatch
        }
    };
}
