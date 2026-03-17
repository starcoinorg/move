// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::VMConfig,
    loader::{Loader, ModuleStorageAdapter},
    RuntimeEnvironment,
};
use move_vm_types::loaded_data::runtime_types::TypeBuilder;

pub(crate) struct ExecutionContext<'a> {
    loader: &'a Loader,
    module_store: &'a ModuleStorageAdapter,
    runtime_environment: RuntimeEnvironment,
}

impl<'a> ExecutionContext<'a> {
    pub(crate) fn new(loader: &'a Loader, module_store: &'a ModuleStorageAdapter) -> Self {
        Self {
            loader,
            module_store,
            runtime_environment: loader.runtime_environment(),
        }
    }

    pub(crate) fn loader(&self) -> &'a Loader {
        self.loader
    }

    pub(crate) fn module_store(&self) -> &'a ModuleStorageAdapter {
        self.module_store
    }

    pub(crate) fn runtime_environment(&self) -> RuntimeEnvironment {
        self.runtime_environment.clone()
    }

    pub(crate) fn runtime_environment_ref(&self) -> &RuntimeEnvironment {
        &self.runtime_environment
    }

    pub(crate) fn vm_config(&self) -> &VMConfig {
        self.loader.vm_config()
    }

    pub(crate) fn ty_builder(&self) -> &TypeBuilder {
        self.loader.ty_builder()
    }
}
