// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::compiler::{as_module, compile_units};
use move_binary_format::{errors::PartialVMResult, file_format::CodeOffset, CompiledModule};
use move_core_types::{
    account_address::AccountAddress,
    gas_algebra::{InternalGas, NumArgs, NumBytes, NumTypeNodes},
    identifier::{IdentStr, Identifier},
    language_storage::{ModuleId, StructTag, TypeTag},
    vm_status::StatusCode,
};
use move_vm_runtime::{
    module_traversal::{TraversalContext, TraversalStorage},
    move_vm::MoveVM,
};
use move_vm_test_utils::InMemoryStorage;
use move_vm_types::{
    gas::{GasMeter, SimpleInstruction, UnmeteredGasMeter},
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

fn compile_module(code: &str) -> CompiledModule {
    let mut units = compile_units(code).unwrap();
    as_module(units.pop().unwrap())
}

fn publish_module(storage: &mut InMemoryStorage, module: &CompiledModule) {
    let mut bytes = vec![];
    module.serialize(&mut bytes).unwrap();
    storage.publish_or_overwrite_module(module.self_id(), bytes);
}

fn struct_type_tag(module_id: &ModuleId, struct_name: &str) -> TypeTag {
    TypeTag::Struct(Box::new(StructTag {
        address: *module_id.address(),
        module: module_id.name().to_owned(),
        name: Identifier::new(struct_name).unwrap(),
        type_args: vec![],
    }))
}

#[test]
fn layout_dependency_metering_on_cache_hit() {
    let addr = AccountAddress::new([42; AccountAddress::LENGTH]);
    let code = format!(
        r#"
        module 0x{}::M {{
            struct R has key {{ v: u64 }}
        }}
    "#,
        addr.to_hex(),
    );
    let module = compile_module(&code);
    let module_id = module.self_id();

    let mut storage = InMemoryStorage::new();
    publish_module(&mut storage, &module);

    let vm = MoveVM::new(vec![]).unwrap();
    let type_tag = struct_type_tag(&module_id, "R");

    let mut meter = TrackingGasMeter::default();
    let traversal_storage = TraversalStorage::new();
    let mut traversal_context = TraversalContext::new(&traversal_storage);
    let mut session = vm.new_session(&storage);
    session
        .get_type_layout_with_metering(&type_tag, &mut meter, &mut traversal_context)
        .unwrap();
    assert_eq!(meter.dependency_modules, vec![module_id.clone()]);

    meter.dependency_modules.clear();
    let traversal_storage = TraversalStorage::new();
    let mut traversal_context = TraversalContext::new(&traversal_storage);
    let mut session = vm.new_session(&storage);
    session
        .get_type_layout_with_metering(&type_tag, &mut meter, &mut traversal_context)
        .unwrap();
    assert_eq!(meter.dependency_modules, vec![module_id]);
}

#[test]
fn layout_depth_limit_exceeded() {
    let addr = AccountAddress::new([1; AccountAddress::LENGTH]);
    let mut nested = "u8".to_string();
    for _ in 0..129 {
        nested = format!("vector<{}>", nested);
    }
    let code = format!(
        r#"
        module 0x{}::M {{
            struct Deep has key {{ v: {} }}
        }}
    "#,
        addr.to_hex(),
        nested,
    );
    let module = compile_module(&code);
    let module_id = module.self_id();

    let mut storage = InMemoryStorage::new();
    publish_module(&mut storage, &module);

    let vm = MoveVM::new(vec![]).unwrap();
    let type_tag = struct_type_tag(&module_id, "Deep");
    let mut session = vm.new_session(&storage);
    let mut gas_meter = UnmeteredGasMeter;
    let traversal_storage = TraversalStorage::new();
    let mut traversal_context = TraversalContext::new(&traversal_storage);
    let err = session
        .get_type_layout_with_metering(&type_tag, &mut gas_meter, &mut traversal_context)
        .unwrap_err();
    assert_eq!(err.major_status(), StatusCode::VM_MAX_VALUE_DEPTH_REACHED);
}

#[test]
fn layout_node_limit_exceeded() {
    let addr = AccountAddress::new([2; AccountAddress::LENGTH]);
    let mut fields = String::new();
    let field_count = 800usize;
    for i in 0..field_count {
        if i > 0 {
            fields.push_str(", ");
        }
        fields.push_str(&format!("f{}: vector<u8>", i));
    }
    let code = format!(
        r#"
        module 0x{}::M {{
            struct Big has key {{ {} }}
        }}
    "#,
        addr.to_hex(),
        fields,
    );
    let module = compile_module(&code);
    let module_id = module.self_id();

    let mut storage = InMemoryStorage::new();
    publish_module(&mut storage, &module);

    let vm = MoveVM::new(vec![]).unwrap();
    let type_tag = struct_type_tag(&module_id, "Big");
    let mut session = vm.new_session(&storage);
    let mut gas_meter = UnmeteredGasMeter;
    let traversal_storage = TraversalStorage::new();
    let mut traversal_context = TraversalContext::new(&traversal_storage);
    let err = session
        .get_type_layout_with_metering(&type_tag, &mut gas_meter, &mut traversal_context)
        .unwrap_err();
    assert_eq!(err.major_status(), StatusCode::TOO_MANY_TYPE_NODES);
}
