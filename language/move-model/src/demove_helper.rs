use std::{borrow::Borrow, collections::BTreeMap, rc::Rc};

use codespan::Files;
use itertools::Itertools;
use move_command_line_common::files::FileHash;
use move_compiler::expansion::ast::Program;

use crate::{
    ast::*,
    builder::{model_builder::ModelBuilder, module_builder::ModuleBuilder},
    expansion_script_to_module,
    model::*,
    symbol::*,
};

impl ModuleData {
    pub fn dummy(name: ModuleName, id: usize) -> ModuleData {
        ModuleData {
            name,
            id: ModuleId::new(id),
            attributes: Default::default(),
            compiled_module: None,
            source_map: None,
            named_constants: Default::default(),
            struct_data: Default::default(),
            struct_idx_to_id: Default::default(),
            function_data: Default::default(),
            function_idx_to_id: Default::default(),
            spec_vars: Default::default(),
            spec_funs: Default::default(),
            module_spec: Default::default(),
            loc: Default::default(),
            spec_block_infos: Default::default(),
            used_modules: Default::default(),
            friend_modules: Default::default(),
        }
    }
}

/*
impl StructData {
    /// Creates a new struct data.
    pub fn dummy(name: Symbol) -> StructData {
        StructData {
            name,
            loc: Default::default(),
            attributes: Default::default(),
            info: Default::default(),
            field_data: Default::default(),
            spec: Default::default(),
        }
    }
}
 */

impl FunctionData {
    /// Creates a new function data.
    pub fn dummy(name: Symbol) -> FunctionData {
        FunctionData {
            name,
            loc: Default::default(),
            def_idx: Default::default(),
            handle_idx: Default::default(),
            attributes: Default::default(),
            arg_names: vec![],
            type_arg_names: vec![],
            spec: Default::default(),
            called_funs: Default::default(),
            calling_funs: Default::default(),
            transitive_closure_of_called_funs: Default::default(),
        }
    }
}

pub fn run_stackless_compiler(env: &mut GlobalEnv, program: Program) {
    env.add_source(FileHash::empty(), Rc::new(BTreeMap::new()), "", "", false);
    (env.file_hash_map).insert(
        FileHash::empty(),
        (
            "".to_string(),
            Files::<String>::default().add("".to_string(), "".to_string()),
        ),
    );

    let mut builder: ModelBuilder<'_> = ModelBuilder::new(env);

    for (module_count, (module_id, module_def)) in program
        .modules
        .into_iter()
        .sorted_by_key(|(_, def)| def.dependency_order)
        .enumerate()
    {
        let loc = builder.to_loc(&module_def.loc);
        let addr_bytes = builder.resolve_address(&loc, &module_id.value.address);
        let module_name = ModuleName::from_address_bytes_and_name(
            addr_bytes,
            builder
                .env
                .symbol_pool()
                .make(&module_id.value.module.0.value),
        );
        let module_id = ModuleId::new(module_count);
        let mut module_translator = ModuleBuilder::new(&mut builder, module_id, module_name);
        // module_translator.translate(loc, module_def, None);
    }
    for (i, (_, script_def)) in program.scripts.into_iter().enumerate() {
        let loc = builder.to_loc(&script_def.loc);

        let module_name = ModuleName::pseudo_script_name(builder.env.symbol_pool(), i);
        let module_id = ModuleId::new(builder.env.module_data.len());
        let mut module_translator = ModuleBuilder::new(&mut builder, module_id, module_name);
        let module_def = expansion_script_to_module(script_def);
        // XXX FIXME YSG
        // module_translator.translate(loc, module_def, None);
    }

    /*
    XXX FIXME YSG
    for module in env.module_data.iter_mut() {
        for fun_data in module.function_data.values_mut() {
            fun_data.called_funs =
                fun_data
                    .def
                    .borrow()
                    .as_ref()
                    .map(|e| e.called_funs())
                    .unwrap_or_default()
        }
    } */
}
