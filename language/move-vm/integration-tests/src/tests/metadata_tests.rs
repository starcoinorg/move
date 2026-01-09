// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::compiler::{as_module, compile_units};
use bytes::Bytes;
use move_binary_format::file_format::CompiledModule;
use move_core_types::{
    account_address::AccountAddress,
    identifier::Identifier,
    language_storage::ModuleId,
    metadata::Metadata,
    resolver::{ModuleResolver, ResourceResolver},
    value::serialize_values,
    vm_status::StatusCode,
};
use move_vm_runtime::{
    module_traversal::{TraversalContext, TraversalStorage},
    move_vm::MoveVM,
};
use move_vm_types::gas::UnmeteredGasMeter;

struct MetadataStorage {
    module_id: ModuleId,
    module_bytes: Bytes,
    expected_key: Vec<u8>,
}

impl ModuleResolver for MetadataStorage {
    type Error = move_binary_format::errors::PartialVMError;

    fn get_module_metadata(&self, _module_id: &ModuleId) -> Vec<Metadata> {
        vec![]
    }

    fn get_module(&self, module_id: &ModuleId) -> Result<Option<Bytes>, Self::Error> {
        if module_id == &self.module_id {
            Ok(Some(self.module_bytes.clone()))
        } else {
            Ok(None)
        }
    }
}

impl ResourceResolver for MetadataStorage {
    type Error = move_binary_format::errors::PartialVMError;

    fn get_resource_bytes_with_metadata_and_layout(
        &self,
        _address: &AccountAddress,
        _tag: &move_core_types::language_storage::StructTag,
        metadata: &[Metadata],
        _maybe_layout: Option<&move_core_types::value::MoveTypeLayout>,
    ) -> Result<(Option<Bytes>, usize), Self::Error> {
        let has_key = metadata.iter().any(|entry| entry.key == self.expected_key);
        if has_key {
            Ok((None, 0))
        } else {
            Err(move_binary_format::errors::PartialVMError::new(
                StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR,
            )
            .with_message("metadata missing for resource read".to_string()))
        }
    }
}

fn compile_module(code: &str) -> CompiledModule {
    let mut units = compile_units(code).unwrap();
    as_module(units.pop().unwrap())
}

#[test]
fn metadata_is_forwarded_for_resource_reads() {
    let addr = AccountAddress::new([9; AccountAddress::LENGTH]);
    let code = format!(
        r#"
        module {}::M {{
            struct R has key {{ }}

            public fun has(addr: address): bool {{
                exists<R>(addr)
            }}
        }}
    "#,
        addr.to_hex_literal(),
    );

    let mut module = compile_module(&code);
    let metadata_key = b"test_metadata_key".to_vec();
    module.metadata.push(Metadata {
        key: metadata_key.clone(),
        value: b"1".to_vec(),
    });

    let mut module_bytes = vec![];
    module.serialize(&mut module_bytes).unwrap();

    let module_id = module.self_id();
    let storage = MetadataStorage {
        module_id: module_id.clone(),
        module_bytes: Bytes::from(module_bytes),
        expected_key: metadata_key,
    };

    let vm = MoveVM::new(vec![]).unwrap();
    let mut session = vm.new_session(&storage);
    let traversal_storage = TraversalStorage::new();
    let mut traversal_context = TraversalContext::new(&traversal_storage);
    let func_name = Identifier::new("has").unwrap();
    let addr_arg = serialize_values(&vec![move_core_types::value::MoveValue::Address(addr)]);

    let result = session.execute_function_bypass_visibility(
        &module_id,
        &func_name,
        vec![],
        addr_arg,
        &mut UnmeteredGasMeter,
        &mut traversal_context,
    );

    assert!(result.is_ok());
}
