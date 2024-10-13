use crate::session::Session;
use move_binary_format::access::ModuleAccess;
use move_binary_format::compatibility::Compatibility;
use move_binary_format::errors::*;
use move_binary_format::{normalized, CompiledModule, IndexKind};
use move_core_types::vm_status::StatusCode;
use move_core_types::{
    account_address::AccountAddress,
    identifier::IdentStr,
    language_storage::{ModuleId, TypeTag},
};
use move_vm_types::gas::GasMeter;
use std::collections::BTreeSet;
use tracing::warn;

/// Publish module bundle options
/// - force_publish: force publish without compatibility check.
/// - only_new_module: cannot only publish new module, update existing modules is not allowed.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct PublishModuleBundleOption {
    pub force_publish: bool,
    pub only_new_module: bool,
}

/// A adapter for wrap MoveVM Session
pub struct SessionAdapter<'r, 'l> {
    pub(crate) session: Session<'r, 'l>,
}

impl<'r, 'l> From<Session<'r, 'l>> for SessionAdapter<'r, 'l> {
    fn from(s: Session<'r, 'l>) -> Self {
        Self { session: s }
    }
}
impl<'r, 'l> Into<Session<'r, 'l>> for SessionAdapter<'r, 'l> {
    fn into(self) -> Session<'r, 'l> {
        self.session
    }
}

impl<'r, 'l> AsRef<Session<'r, 'l>> for SessionAdapter<'r, 'l> {
    fn as_ref(&self) -> &Session<'r, 'l> {
        &self.session
    }
}

impl<'r, 'l> AsMut<Session<'r, 'l>> for SessionAdapter<'r, 'l> {
    fn as_mut(&mut self) -> &mut Session<'r, 'l> {
        &mut self.session
    }
}

impl<'r, 'l> SessionAdapter<'r, 'l> {
    pub fn new(session: Session<'r, 'l>) -> Self {
        Self { session }
    }

    /// Publish module bundle with custom option.
    /// The code is copied from `VMRuntime::publish_module_bundle` with modification to support ModuleBundleVerifyOption.
    pub fn publish_module_bundle_with_option(
        &mut self,
        modules: Vec<Vec<u8>>,
        sender: AccountAddress,
        gas_meter: &mut impl GasMeter,
        option: PublishModuleBundleOption,
    ) -> VMResult<()> {
        let compiled_modules =
            self.verify_module_bundle(modules.clone(), sender, gas_meter, option)?;

        let data_store = &mut self.session.data_cache;
        let mut clean_cache = false;
        // All modules verified, publish them to data cache
        for (module, blob) in compiled_modules.into_iter().zip(modules.into_iter()) {
            let republish = if data_store.exists_module(&module.self_id())? {
                clean_cache = true;
                true
            } else {
                false
            };
            data_store.publish_module(&module.self_id(), blob, republish)?;
        }
        if clean_cache {
            self.session.move_vm.runtime.loader.mark_as_invalid();
            self.session.move_vm.runtime.loader.flush_if_invalidated();
        }
        Ok(())
    }

    /// Verify module bundle.
    /// The code is copied from `move_vm:::VMRuntime::publish_module_bundle` with modification to support ModuleBundleVerifyOption.
    pub fn verify_module_bundle(
        &mut self,
        modules: Vec<Vec<u8>>,
        sender: AccountAddress,
        _gas_meter: &mut impl GasMeter,
        option: PublishModuleBundleOption,
    ) -> VMResult<Vec<CompiledModule>> {
        let data_store = &self.session.data_cache;

        // deserialize the modules. Perform bounds check. After this indexes can be
        // used with the `[]` operator
        let compiled_modules = match modules
            .iter()
            .map(|blob| CompiledModule::deserialize(blob))
            .collect::<PartialVMResult<Vec<_>>>()
        {
            Ok(modules) => modules,
            Err(err) => {
                warn!("[VM] module deserialization failed {:?}", err);
                return Err(err.finish(Location::Undefined));
            }
        };

        // Make sure all modules' self addresses matches the transaction sender. The self address is
        // where the module will actually be published. If we did not check this, the sender could
        // publish a module under anyone's account.
        for module in &compiled_modules {
            if module.address() != &sender {
                return Err(verification_error(
                    StatusCode::MODULE_ADDRESS_DOES_NOT_MATCH_SENDER,
                    IndexKind::AddressIdentifier,
                    module.self_handle_idx().0,
                )
                .finish(Location::Undefined));
            }
        }

        // Collect ids for modules that are published together
        let mut bundle_unverified = BTreeSet::new();

        // For now, we assume that all modules can be republished, as long as the new module is
        // backward compatible with the old module.
        //
        // TODO: in the future, we may want to add restrictions on module republishing, possibly by
        // changing the bytecode format to include an `is_upgradable` flag in the CompiledModule.
        for module in &compiled_modules {
            let module_id = module.self_id();
            if data_store.exists_module(&module_id)? {
                if option.only_new_module {
                    warn!(
                        "[VM] module {:?} already exists. Only allow publish new modules",
                        module_id
                    );
                    return Err(PartialVMError::new(StatusCode::INVALID_MODULE_PUBLISHER)
                        .at_index(IndexKind::ModuleHandle, module.self_handle_idx().0)
                        .finish(Location::Undefined));
                }

                let old_module = self
                    .session
                    .load_module(&module_id)
                    .map(|module| {
                        CompiledModule::deserialize_with_config(
                            &module,
                            &self.session.get_vm_config().deserializer_config,
                        )
                    })?
                    .map_err(|err| err.finish(Location::Undefined))?;
                let old_m = normalized::Module::new(&old_module);
                let new_m = normalized::Module::new(&module);
                if Compatibility::new(true, false)
                    .check(&old_m, &new_m)
                    .is_err()
                    && !option.force_publish
                {
                    return Err(PartialVMError::new(
                        StatusCode::BACKWARD_INCOMPATIBLE_MODULE_UPDATE,
                    )
                    .finish(Location::Undefined));
                }
            }
            if !bundle_unverified.insert(module_id) {
                return Err(PartialVMError::new(StatusCode::DUPLICATE_MODULE_NAME)
                    .finish(Location::Undefined));
            }
        }

        let vm = self.session.get_move_vm();
        // Perform bytecode and loading verification. Modules must be sorted in topological order.
        let data_store = &mut self.session.data_cache;

        vm.runtime.loader.verify_module_bundle_for_publication(
            &compiled_modules,
            data_store,
            &self.session.module_store,
        )?;

        Ok(compiled_modules)
    }

    pub fn verify_script_args(
        &mut self,
        _script: Vec<u8>,
        _ty_args: Vec<TypeTag>,
        _args: Vec<Vec<u8>>,
        _senders: Vec<AccountAddress>,
    ) -> VMResult<()> {
        // load the script, perform verification
        // let (main, _ty_args, params) = self.session.runtime.loader.load_script(
        //     &script,
        //     &ty_args,
        //     &mut self.session.data_cache,
        // )?;
        // let _signers_and_args = self
        //     .session
        //     .runtime
        //     .create_signers_and_arguments(main.file_format_version(), &params, senders, args)
        //     .map_err(|err| err.finish(Location::Undefined))?;
        Ok(())
    }

    // FIXME: i don't know how to fix this.
    pub fn verify_script_function_args(
        &mut self,
        _module: &ModuleId,
        _function_name: &IdentStr,
        _ty_args: Vec<TypeTag>,
        _args: Vec<Vec<u8>>,
        _senders: Vec<AccountAddress>,
    ) -> VMResult<()> {
        // let (func, ty_args, params, _return_tys) = self.session.runtime.loader.load_function(
        //     function_name,
        //     module,
        //     &ty_args,
        //     &mut self.session.data_cache,
        // )?;
        // let params = params
        //     .into_iter()
        //     .map(|ty| ty.subst(&ty_args))
        //     .collect::<PartialVMResult<Vec<_>>>()
        //     .map_err(|err| err.finish(Location::Undefined))?;

        // let _signer_and_args = self
        //     .session
        //     .runtime
        //     .create_signers_and_arguments(func.file_format_version(), &params, senders, args)
        //     .map_err(|err| err.finish(Location::Undefined))?;
        Ok(())
    }

    /// Clear vm runtimer loader's cache to reload new modules from state cache
    pub fn empty_loader_cache(&self) -> VMResult<()> {
        self.session.get_move_vm().runtime.loader.mark_as_invalid();
        self.session
            .get_move_vm()
            .runtime
            .loader
            .flush_if_invalidated();
        Ok(())
    }
}
