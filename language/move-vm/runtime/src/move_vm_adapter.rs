use move_binary_format::errors::*;
use move_core_types::vm_status::StatusCode;
// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::loader::Function;
use crate::session::{LoadedFunctionInstantiation, Session};
use move_binary_format::{
    access::ModuleAccess, compatibility::Compatibility, normalized, CompiledModule,
    IndexKind,
};
use move_core_types::value::MoveValue;
use move_core_types::{
    account_address::AccountAddress,
    identifier::IdentStr,
    language_storage::{ModuleId, TypeTag},
    resolver::*,
};
use move_vm_types::data_store::DataStore;
use move_vm_types::gas::GasMeter;
use move_vm_types::loaded_data::runtime_types::Type;
use std::collections::BTreeSet;
use std::sync::Arc;
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
pub struct SessionAdapter<'r, 'l, R> {
    pub(crate) session: Session<'r, 'l, R>,
}

impl<'r, 'l, R> From<Session<'r, 'l, R>> for SessionAdapter<'r, 'l, R> {
    fn from(s: Session<'r, 'l, R>) -> Self {
        Self { session: s }
    }
}

impl<'r, 'l, R> Into<Session<'r, 'l, R>> for SessionAdapter<'r, 'l, R> {
    fn into(self) -> Session<'r, 'l, R> {
        self.session
    }
}

impl<'r, 'l, R> AsRef<Session<'r, 'l, R>> for SessionAdapter<'r, 'l, R> {
    fn as_ref(&self) -> &Session<'r, 'l, R> {
        &self.session
    }
}

impl<'r, 'l, R> AsMut<Session<'r, 'l, R>> for SessionAdapter<'r, 'l, R> {
    fn as_mut(&mut self) -> &mut Session<'r, 'l, R> {
        &mut self.session
    }
}

impl<'r, 'l, R: MoveResolver> SessionAdapter<'r, 'l, R> {
    pub fn new(session: Session<'r, 'l, R>) -> Self {
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
        // All modules verified, publish them to data cache
        for (module, blob) in compiled_modules.into_iter().zip(modules.into_iter()) {
            let republish = if data_store.exists_module(&module.self_id())? {
                true
            } else {
                false
            };
            data_store.publish_module(&module.self_id(), blob, republish)?;
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
        let data_store = &mut self.session.data_cache;

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

                let old_module_ref = self
                    .session
                    .runtime
                    .loader
                    .load_module(&module_id, data_store)?;
                let old_module = old_module_ref.module();
                let old_m = normalized::Module::new(old_module);
                let new_m = normalized::Module::new(&module);
                let compat = Compatibility::check(false, &old_m, &new_m);
                if !compat.is_fully_compatible() && !option.force_publish {
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

        // Perform bytecode and loading verification. Modules must be sorted in topological order.
        self.session
            .runtime
            .loader
            .verify_module_bundle_for_publication(&compiled_modules, data_store)?;
        Ok(compiled_modules)
    }

    pub fn verify_script_args(
        &mut self,
        script: Vec<u8>,
        ty_args: Vec<TypeTag>,
        args: Vec<Vec<u8>>,
        sender: AccountAddress,
    ) -> VMResult<()> {
        //load the script, perform verification
        let (
            main,
            LoadedFunctionInstantiation {
                type_arguments: _,
                parameters,
                return_,
            },
        ) = self
            .session
            .runtime
            .loader
            .load_script(&script, &ty_args, &self.session.data_cache)?;

        Self::check_script_return(return_)?;

        self.check_script_signer_and_build_args(main, parameters, args, sender)?;

        Ok(())
    }

    pub fn verify_script_function_args(
        &mut self,
        module: &ModuleId,
        function_name: &IdentStr,
        ty_args: Vec<TypeTag>,
        args: Vec<Vec<u8>>,
        sender: AccountAddress,
    ) -> VMResult<()> {
        let (
            _module,
            func,
            LoadedFunctionInstantiation {
                type_arguments: _,
                parameters,
                return_,
            },
        ) = self.session.runtime.loader.load_function(
            module,
            function_name,
            &ty_args,
            &self.session.data_cache,
        )?;

        Self::check_script_return(return_)?;

        self.check_script_signer_and_build_args(func, parameters, args, sender)?;

        Ok(())
    }

    //ensure the script function not return value
    fn check_script_return(return_: Vec<Type>) -> VMResult<()> {
        return if !return_.is_empty() {
            Err(PartialVMError::new(StatusCode::RET_TYPE_MISMATCH_ERROR)
                .with_message(format!(
                    "Expected script function should not return value, but got {:?}",
                    return_
                ))
                .finish(Location::Undefined))
        } else {
            Ok(())
        };
    }

    fn check_script_signer_and_build_args(
        &self,
        func: Arc<Function>,
        arg_tys: Vec<Type>,
        args: Vec<Vec<u8>>,
        sender: AccountAddress,
    ) -> VMResult<()> {
        let mut has_signer = false;
        for (i, param_signature) in func.parameters().0.iter().enumerate() {
            //If script function contains signer parameter, it must been the first one.
            //Move has release the signer check at https://github.com/move-language/move/commit/086005ed6a2e54f2f971b77e092416a1f9248a5b
            //We should add check check at the compile time.
            if param_signature.is_signer() {
                if i == 0 {
                    has_signer = true;
                } else {
                    return Err(PartialVMError::new(
                        StatusCode::NUMBER_OF_SIGNER_ARGUMENTS_MISMATCH,
                    )
                    .with_message(format!(
                        "Expected signer arg is this first arg, but got it at {}",
                        i + 1
                    ))
                    .finish(Location::Undefined));
                }
            }
        }

        let final_args = if has_signer {
            let signer = MoveValue::Signer(sender);
            let mut final_args = vec![signer
                .simple_serialize()
                .expect("serialize signer should success")];
            final_args.extend(args);
            final_args
        } else {
            args
        };

        let (_, _) = self
            .session
            .runtime
            .deserialize_args(arg_tys, final_args)
            .map_err(|err| err.finish(Location::Undefined))?;

        Ok(())
    }

    /// Clear vm runtimer loader's cache to reload new modules from state cache
    pub fn empty_loader_cache(&self) -> VMResult<()> {
        self.session.runtime.loader.empty_cache()
    }
}
