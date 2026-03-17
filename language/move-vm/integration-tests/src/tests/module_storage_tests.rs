// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::compiler::{compile_modules_in_file, compile_units};
use move_binary_format::file_format::{
    empty_module, empty_script, AddressIdentifierIndex, CompiledModule, IdentifierIndex,
    ModuleHandle,
};
use move_compiler::compiled_unit::AnnotatedCompiledUnit;
use move_core_types::{
    account_address::AccountAddress, identifier::Identifier, language_storage::ModuleId,
    vm_status::StatusCode,
};
use move_vm_runtime::{
    config::VMConfig,
    module_traversal::{TraversalContext, TraversalStorage},
    AsUnsyncCodeStorage, AsUnsyncModuleStorage, EagerLoader, FunctionDefinitionLoader, LazyLoader,
    LegacyLoaderConfig, ModuleMetadataLoader, ModuleStorage, RuntimeEnvironment, ScriptLoader,
};
use move_vm_test_utils::InMemoryStorage;
use move_vm_types::{
    code::{Code, ScriptCache},
    gas::UnmeteredGasMeter,
    sha3_256,
};
use std::path::PathBuf;

const WORKING_ACCOUNT: AccountAddress = AccountAddress::TWO;

fn initialize_storage(enable_lazy_loading: bool) -> InMemoryStorage {
    initialize_storage_with_modules(enable_lazy_loading, get_modules())
}

fn initialize_storage_with_modules(
    enable_lazy_loading: bool,
    modules: Vec<CompiledModule>,
) -> InMemoryStorage {
    let runtime_environment = RuntimeEnvironment::new_with_config(
        vec![],
        VMConfig {
            enable_lazy_loading,
            ..Default::default()
        },
    );
    let mut storage = InMemoryStorage::new_with_runtime_environment(runtime_environment);
    add_modules_to_storage(&mut storage, modules);
    storage
}

fn add_modules_to_storage(storage: &mut InMemoryStorage, modules: Vec<CompiledModule>) {
    for module in modules {
        let mut bytes = vec![];
        let module_id = module.self_id().clone();
        module.serialize(&mut bytes).unwrap();
        storage.publish_or_overwrite_module(module_id, bytes);
    }
}

fn get_modules() -> Vec<CompiledModule> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("src/tests/loader_tests_modules.move");
    compile_modules_in_file(&path).unwrap()
}

fn compile_entry_f_script() -> Vec<u8> {
    let units = compile_units(
        r#"
address 0x2 {
    module F {
        public fun entry_f() {}
    }
}

script {
    fun main() {
        0x2::F::entry_f();
    }
}
"#,
    )
    .unwrap();

    let script = units
        .into_iter()
        .find_map(|unit| match unit {
            AnnotatedCompiledUnit::Script(script) => Some(script.named_script.script),
            AnnotatedCompiledUnit::Module(_) => None,
        })
        .expect("script unit must exist");

    let mut bytes = vec![];
    script.serialize(&mut bytes).unwrap();
    bytes
}

fn module_id(name: &str) -> ModuleId {
    ModuleId::new(WORKING_ACCOUNT, Identifier::new(name).unwrap())
}

fn entry_function(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn empty_test_module(name: &str, deps: &[&str], friends: &[&str]) -> CompiledModule {
    let mut module = empty_module();
    module.address_identifiers[0] = WORKING_ACCOUNT;
    module.identifiers[0] = Identifier::new(name).unwrap();

    for dep in deps {
        let ident_idx = IdentifierIndex(module.identifiers.len() as u16);
        module.identifiers.push(Identifier::new(*dep).unwrap());
        module.module_handles.push(ModuleHandle {
            address: AddressIdentifierIndex(0),
            name: ident_idx,
        });
    }

    for friend in friends {
        let ident_idx = IdentifierIndex(module.identifiers.len() as u16);
        module.identifiers.push(Identifier::new(*friend).unwrap());
        module.friend_decls.push(ModuleHandle {
            address: AddressIdentifierIndex(0),
            name: ident_idx,
        });
    }

    module
}

fn empty_test_script_with_dependencies(deps: &[&str]) -> Vec<u8> {
    let mut script = empty_script();
    script.address_identifiers.push(WORKING_ACCOUNT);

    for dep in deps {
        let ident_idx = IdentifierIndex(script.identifiers.len() as u16);
        script.identifiers.push(Identifier::new(*dep).unwrap());
        script.module_handles.push(ModuleHandle {
            address: AddressIdentifierIndex(0),
            name: ident_idx,
        });
    }

    let mut bytes = vec![];
    script.serialize(&mut bytes).unwrap();
    bytes
}

fn assert_script_cache_state<C>(
    code_storage: &C,
    deserialized: Vec<&[u8; 32]>,
    verified: Vec<&[u8; 32]>,
) where
    C: ScriptCache<Key = [u8; 32]>,
{
    assert_eq!(
        code_storage.num_scripts(),
        deserialized.len() + verified.len()
    );
    for hash in deserialized {
        assert!(matches!(
            code_storage.get_script(hash),
            Some(Code::Deserialized(_))
        ));
    }
    for hash in verified {
        assert!(matches!(
            code_storage.get_script(hash),
            Some(Code::Verified(_))
        ));
    }
}

fn assert_cyclic_load_error(status: StatusCode) {
    assert!(
        matches!(
            status,
            StatusCode::CYCLIC_MODULE_DEPENDENCY | StatusCode::UNEXPECTED_VERIFIER_ERROR
        ),
        "unexpected status for cyclic dependency load: {status:?}"
    );
}

fn assert_module_metadata_only_deserializes_requested_module(enable_lazy_loading: bool) {
    let storage = initialize_storage(enable_lazy_loading);
    let module_storage = storage.as_unsync_module_storage();
    let module_id = module_id("F");
    let traversal_storage = TraversalStorage::new();

    if enable_lazy_loading {
        LazyLoader::new(&module_storage)
            .load_module_for_metadata(
                &mut UnmeteredGasMeter,
                &mut TraversalContext::new(&traversal_storage),
                &module_id,
            )
            .unwrap();
    } else {
        EagerLoader::new(&module_storage)
            .load_module_for_metadata(
                &mut UnmeteredGasMeter,
                &mut TraversalContext::new(&traversal_storage),
                &module_id,
            )
            .unwrap();
    }

    module_storage.assert_cached_state(vec![&module_id], vec![]);
}

#[test]
fn missing_module_does_not_cache_any_state() {
    let storage = initialize_storage_with_modules(false, vec![]);
    let module_storage = storage.as_unsync_module_storage();

    assert!(!module_storage
        .unmetered_check_module_exists(module_id("Missing").address(), module_id("Missing").name())
        .unwrap());
    module_storage.assert_cached_state(vec![], vec![]);
}

#[test]
fn module_exists_caches_only_deserialized_state() {
    let storage = initialize_storage_with_modules(false, vec![empty_test_module("A", &[], &[])]);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");

    assert!(module_storage
        .unmetered_check_module_exists(module_a.address(), module_a.name())
        .unwrap());
    module_storage.assert_cached_state(vec![&module_a], vec![]);
}

#[test]
fn eager_metadata_only_deserializes_requested_module() {
    assert_module_metadata_only_deserializes_requested_module(false);
}

#[test]
fn lazy_metadata_only_deserializes_requested_module() {
    assert_module_metadata_only_deserializes_requested_module(true);
}

#[test]
fn eager_function_load_verifies_transitive_dependency_closure() {
    let storage = initialize_storage(false);
    let module_storage = storage.as_unsync_module_storage();
    let module_f = module_id("F");
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let traversal_storage = TraversalStorage::new();

    EagerLoader::new(&module_storage)
        .load_function_definition(
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &module_f,
            entry_function("entry_f").as_ident_str(),
        )
        .unwrap();

    module_storage.assert_cached_state(vec![], vec![&module_f, &module_a, &module_b, &module_c]);
}

#[test]
fn lazy_function_load_verifies_only_requested_module() {
    let storage = initialize_storage(true);
    let module_storage = storage.as_unsync_module_storage();
    let module_f = module_id("F");
    let traversal_storage = TraversalStorage::new();

    LazyLoader::new(&module_storage)
        .load_function_definition(
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &module_f,
            entry_function("entry_f").as_ident_str(),
        )
        .unwrap();

    module_storage.assert_cached_state(vec![], vec![&module_f]);
}

#[test]
fn eager_script_load_verifies_dependency_closure() {
    let storage = initialize_storage(false);
    let code_storage = storage.as_unsync_code_storage();
    let module_f = module_id("F");
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let script = compile_entry_f_script();
    let hash = sha3_256(&script);
    let traversal_storage = TraversalStorage::new();

    EagerLoader::new(&code_storage)
        .load_script(
            &LegacyLoaderConfig::unmetered(),
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &script,
            &[],
        )
        .unwrap();

    assert!(code_storage.get_verified_script(&hash).is_some());
    code_storage
        .module_storage()
        .assert_cached_state(vec![], vec![&module_f, &module_a, &module_b, &module_c]);
}

#[test]
fn lazy_script_load_keeps_transitive_dependencies_unloaded() {
    let storage = initialize_storage(true);
    let code_storage = storage.as_unsync_code_storage();
    let module_f = module_id("F");
    let script = compile_entry_f_script();
    let hash = sha3_256(&script);
    let traversal_storage = TraversalStorage::new();

    LazyLoader::new(&code_storage)
        .load_script(
            &LegacyLoaderConfig::unmetered(),
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &script,
            &[],
        )
        .unwrap();

    assert!(code_storage.get_verified_script(&hash).is_some());
    code_storage
        .module_storage()
        .assert_cached_state(vec![], vec![&module_f]);
}

#[test]
fn load_module_tree_deserialized_caches_only_requested_root() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &["E"], &[]),
        empty_test_module("D", &[], &[]),
        empty_test_module("E", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");

    module_storage
        .unmetered_get_existing_deserialized_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(vec![&module_a], vec![]);
}

#[test]
fn eager_load_module_tree_verifies_entire_closure() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &["E"], &[]),
        empty_test_module("D", &[], &[]),
        empty_test_module("E", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let module_d = module_id("D");
    let module_e = module_id("E");

    module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(
        vec![],
        vec![&module_a, &module_b, &module_c, &module_d, &module_e],
    );
}

#[test]
fn lazy_load_module_tree_verifies_only_requested_root() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &["E"], &[]),
        empty_test_module("D", &[], &[]),
        empty_test_module("E", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(true, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");

    module_storage
        .unmetered_get_existing_lazily_verified_module(&module_a)
        .unwrap();
    module_storage.assert_cached_state(vec![], vec![&module_a]);
}

#[test]
fn eager_load_module_dag_verifies_shared_dependency_once() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &["D"], &[]),
        empty_test_module("D", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let module_d = module_id("D");

    module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(vec![], vec![&module_a, &module_b, &module_c, &module_d]);
}

#[test]
fn eager_load_module_with_friends_keeps_friends_unloaded() {
    let modules = vec![
        empty_test_module("A", &[], &["B", "C"]),
        empty_test_module("B", &[], &[]),
        empty_test_module("C", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");

    module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(vec![], vec![&module_a]);
}

#[test]
fn lazy_load_module_with_friends_keeps_friends_unloaded() {
    let modules = vec![
        empty_test_module("A", &[], &["B", "C"]),
        empty_test_module("B", &[], &[]),
        empty_test_module("C", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(true, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");

    module_storage
        .unmetered_get_existing_lazily_verified_module(&module_a)
        .unwrap();
    module_storage.assert_cached_state(vec![], vec![&module_a]);
}

#[test]
fn eager_load_module_cyclic_dependencies_fails_and_leaves_deserialized_cache() {
    let modules = vec![
        empty_test_module("A", &["B"], &[]),
        empty_test_module("B", &["A"], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");

    let err = module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap_err();
    assert_cyclic_load_error(err.major_status());
    module_storage.assert_cached_state(vec![&module_a, &module_b], vec![]);
}

#[test]
fn eager_load_module_cyclic_dependencies_fails_after_root_deserialized() {
    let modules = vec![
        empty_test_module("A", &["B"], &[]),
        empty_test_module("B", &["A"], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");

    module_storage
        .unmetered_get_existing_deserialized_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(vec![&module_a], vec![]);

    let err = module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap_err();
    assert_cyclic_load_error(err.major_status());
    module_storage.assert_cached_state(vec![&module_a, &module_b], vec![]);
}

#[test]
fn load_module_mixed_deserialized_and_verified_state() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &[], &[]),
        empty_test_module("D", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let module_storage = storage.as_unsync_module_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let module_d = module_id("D");

    module_storage
        .unmetered_get_existing_deserialized_module(module_c.address(), module_c.name())
        .unwrap();
    module_storage
        .unmetered_get_existing_eagerly_verified_module(module_a.address(), module_a.name())
        .unwrap();
    module_storage.assert_cached_state(vec![], vec![&module_a, &module_b, &module_c, &module_d]);
}

#[test]
fn load_script_with_no_dependencies_only_caches_script() {
    let storage = initialize_storage_with_modules(false, vec![]);
    let code_storage = storage.as_unsync_code_storage();
    let script = empty_test_script_with_dependencies(&[]);
    let hash = sha3_256(&script);
    let traversal_storage = TraversalStorage::new();

    EagerLoader::new(&code_storage)
        .load_script(
            &LegacyLoaderConfig::unmetered(),
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &script,
            &[],
        )
        .unwrap();

    assert_script_cache_state(&code_storage, vec![], vec![&hash]);
    code_storage
        .module_storage()
        .assert_cached_state(vec![], vec![]);
}

#[test]
fn eager_load_script_tree_verifies_dependency_closure() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &[], &[]),
        empty_test_module("D", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let code_storage = storage.as_unsync_code_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");
    let module_c = module_id("C");
    let module_d = module_id("D");
    let script = empty_test_script_with_dependencies(&["A"]);
    let hash = sha3_256(&script);
    let traversal_storage = TraversalStorage::new();

    EagerLoader::new(&code_storage)
        .load_script(
            &LegacyLoaderConfig::unmetered(),
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &script,
            &[],
        )
        .unwrap();

    assert_script_cache_state(&code_storage, vec![], vec![&hash]);
    code_storage
        .module_storage()
        .assert_cached_state(vec![], vec![&module_a, &module_b, &module_c, &module_d]);
}

#[test]
fn lazy_load_script_tree_verifies_only_direct_dependency() {
    let modules = vec![
        empty_test_module("A", &["B", "C"], &[]),
        empty_test_module("B", &["D"], &[]),
        empty_test_module("C", &[], &[]),
        empty_test_module("D", &[], &[]),
    ];
    let storage = initialize_storage_with_modules(true, modules);
    let code_storage = storage.as_unsync_code_storage();
    let module_a = module_id("A");
    let script = empty_test_script_with_dependencies(&["A"]);
    let hash = sha3_256(&script);
    let traversal_storage = TraversalStorage::new();

    LazyLoader::new(&code_storage)
        .load_script(
            &LegacyLoaderConfig::unmetered(),
            &mut UnmeteredGasMeter,
            &mut TraversalContext::new(&traversal_storage),
            &script,
            &[],
        )
        .unwrap();

    assert_script_cache_state(&code_storage, vec![], vec![&hash]);
    code_storage
        .module_storage()
        .assert_cached_state(vec![], vec![&module_a]);
}

#[test]
fn eager_load_script_cyclic_dependencies_fails_and_leaves_module_cache_deserialized() {
    let modules = vec![
        empty_test_module("A", &["B"], &[]),
        empty_test_module("B", &["A"], &[]),
    ];
    let storage = initialize_storage_with_modules(false, modules);
    let code_storage = storage.as_unsync_code_storage();
    let module_a = module_id("A");
    let module_b = module_id("B");
    let script = empty_test_script_with_dependencies(&["A"]);
    let traversal_storage = TraversalStorage::new();

    let err = match EagerLoader::new(&code_storage).load_script(
        &LegacyLoaderConfig::unmetered(),
        &mut UnmeteredGasMeter,
        &mut TraversalContext::new(&traversal_storage),
        &script,
        &[],
    ) {
        Ok(_) => panic!("cyclic dependencies should fail"),
        Err(err) => err,
    };
    assert_cyclic_load_error(err.major_status());
    assert_script_cache_state(&code_storage, vec![], vec![]);
    code_storage
        .module_storage()
        .assert_cached_state(vec![&module_a, &module_b], vec![]);
}
