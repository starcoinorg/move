// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::compiler::{as_module, as_script, compile_units};
use move_binary_format::{errors::PartialVMResult, file_format::CodeOffset};
use move_core_types::{
    account_address::AccountAddress,
    gas_algebra::{InternalGas, NumArgs, NumBytes, NumTypeNodes},
    identifier::IdentStr,
    language_storage::ModuleId,
};
use move_compiler::compiled_unit::AnnotatedCompiledUnit;
use move_vm_runtime::{
    module_traversal::{TraversalContext, TraversalStorage},
    move_vm::MoveVM,
};
use move_vm_test_utils::InMemoryStorage;
use move_vm_types::{
    gas::{GasMeter, SimpleInstruction},
    views::{TypeView, ValueView},
};

#[derive(Default)]
struct TrackingGasMeter {
    dependency_modules: Vec<ModuleId>,
}

impl GasMeter for TrackingGasMeter {
    fn balance_internal(&self) -> InternalGas {
        InternalGas::new(0)
    }

    fn charge_simple_instr(&mut self, _instr: SimpleInstruction) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_br_true(&mut self, _target_offset: Option<CodeOffset>) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_br_false(&mut self, _target_offset: Option<CodeOffset>) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_branch(&mut self, _target_offset: CodeOffset) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_pop(&mut self, _popped_val: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_call(
        &mut self,
        _module_id: &ModuleId,
        _func_name: &str,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
        _num_locals: NumArgs,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_call_generic(
        &mut self,
        _module_id: &ModuleId,
        _func_name: &str,
        _ty_args: impl ExactSizeIterator<Item = impl TypeView> + Clone,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
        _num_locals: NumArgs,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_ld_const(&mut self, _size: NumBytes) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_ld_const_after_deserialization(
        &mut self,
        _val: impl ValueView,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_copy_loc(&mut self, _val: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_move_loc(&mut self, _val: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_store_loc(&mut self, _val: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_pack(
        &mut self,
        _is_generic: bool,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_unpack(
        &mut self,
        _is_generic: bool,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_read_ref(&mut self, _val: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_write_ref(
        &mut self,
        _new_val: impl ValueView,
        _old_val: impl ValueView,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_eq(&mut self, _lhs: impl ValueView, _rhs: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_neq(&mut self, _lhs: impl ValueView, _rhs: impl ValueView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_borrow_global(
        &mut self,
        _is_mut: bool,
        _is_generic: bool,
        _ty: impl TypeView,
        _is_success: bool,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_exists(
        &mut self,
        _is_generic: bool,
        _ty: impl TypeView,
        _exists: bool,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_move_from(
        &mut self,
        _is_generic: bool,
        _ty: impl TypeView,
        _val: Option<impl ValueView>,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_move_to(
        &mut self,
        _is_generic: bool,
        _ty: impl TypeView,
        _val: impl ValueView,
        _is_success: bool,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_pack<'a>(
        &mut self,
        _ty: impl TypeView + 'a,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_len(&mut self, _ty: impl TypeView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_borrow(
        &mut self,
        _is_mut: bool,
        _ty: impl TypeView,
        _is_success: bool,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_push_back(
        &mut self,
        _ty: impl TypeView,
        _val: impl ValueView,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_pop_back(
        &mut self,
        _ty: impl TypeView,
        _val: Option<impl ValueView>,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_unpack(
        &mut self,
        _ty: impl TypeView,
        _expect_num_elements: NumArgs,
        _elems: impl ExactSizeIterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_vec_swap(&mut self, _ty: impl TypeView) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_load_resource(
        &mut self,
        _addr: AccountAddress,
        _ty: impl TypeView,
        _val: Option<impl ValueView>,
        _bytes_loaded: NumBytes,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_native_function(
        &mut self,
        _amount: InternalGas,
        _ret_vals: Option<impl ExactSizeIterator<Item = impl ValueView> + Clone>,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_native_function_before_execution(
        &mut self,
        _ty_args: impl ExactSizeIterator<Item = impl TypeView> + Clone,
        _args: impl ExactSizeIterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_drop_frame(
        &mut self,
        _locals: impl Iterator<Item = impl ValueView> + Clone,
    ) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_create_ty(&mut self, _num_nodes: NumTypeNodes) -> PartialVMResult<()> {
        Ok(())
    }

    fn charge_dependency(
        &mut self,
        _is_new: bool,
        addr: &AccountAddress,
        name: &IdentStr,
        _size: NumBytes,
    ) -> PartialVMResult<()> {
        self.dependency_modules
            .push(ModuleId::new(*addr, name.to_owned()));
        Ok(())
    }
}

#[test]
fn script_dependency_metering_on_cache_hit() {
    let addr = AccountAddress::new([42; AccountAddress::LENGTH]);
    let code = format!(
        r#"
        module 0x{}::Dep {{
            public fun foo() {{ }}
        }}

        script {{
            fun main() {{
                0x{}::Dep::foo();
            }}
        }}
    "#,
        addr.to_hex(),
        addr.to_hex()
    );

    let mut module = None;
    let mut script = None;
    for unit in compile_units(&code).unwrap() {
        match unit {
            AnnotatedCompiledUnit::Module(_) => module = Some(as_module(unit)),
            AnnotatedCompiledUnit::Script(_) => script = Some(as_script(unit)),
        }
    }
    let module = module.expect("module is missing");
    let script = script.expect("script is missing");

    let mut module_bytes = vec![];
    module.serialize(&mut module_bytes).unwrap();

    let mut script_bytes = vec![];
    script.serialize(&mut script_bytes).unwrap();

    let module_id = module.self_id();
    let mut storage = InMemoryStorage::new();
    storage.publish_or_overwrite_module(module_id.clone(), module_bytes);

    let vm = MoveVM::new(vec![]).unwrap();
    let mut run = |meter: &mut TrackingGasMeter| {
        let mut session = vm.new_session(&storage);
        let traversal_storage = TraversalStorage::new();
        let mut traversal_context = TraversalContext::new(&traversal_storage);
        session
            .execute_script(
                script_bytes.clone(),
                vec![],
                Vec::<Vec<u8>>::new(),
                meter,
                &mut traversal_context,
            )
            .unwrap();
        session.finish().unwrap();
    };

    let mut meter = TrackingGasMeter::default();
    run(&mut meter);
    assert_eq!(meter.dependency_modules.len(), 1);
    assert_eq!(meter.dependency_modules[0], module_id);

    meter.dependency_modules.clear();
    run(&mut meter);
    assert_eq!(meter.dependency_modules.len(), 1);
    assert_eq!(meter.dependency_modules[0], module_id);
}
