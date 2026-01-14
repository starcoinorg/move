// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::VMConfig,
    loader::{Loader, ModuleCache, ModuleStorage},
    native_functions::{NativeFunction, NativeFunctions},
};
use move_binary_format::errors::PartialVMResult;
use move_core_types::{account_address::AccountAddress, identifier::Identifier};
use std::sync::Arc;

pub struct RuntimeEnvironment {
    loader: Loader,
    module_cache: Arc<ModuleCache>,
}

impl RuntimeEnvironment {
    pub fn new(
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
        vm_config: VMConfig,
    ) -> PartialVMResult<Self> {
        Ok(Self {
            loader: Loader::new(NativeFunctions::new(natives)?, vm_config),
            module_cache: Arc::new(ModuleCache::new()),
        })
    }

    pub fn vm_config(&self) -> &VMConfig {
        self.loader.vm_config()
    }

    pub(crate) fn loader(&self) -> &Loader {
        &self.loader
    }

    pub(crate) fn module_storage(&self) -> Arc<dyn ModuleStorage> {
        self.module_cache.clone() as Arc<dyn ModuleStorage>
    }

    pub(crate) fn module_cache(&self) -> &Arc<ModuleCache> {
        &self.module_cache
    }
}

pub trait WithRuntimeEnvironment {
    fn runtime_environment(&self) -> &RuntimeEnvironment;
}
