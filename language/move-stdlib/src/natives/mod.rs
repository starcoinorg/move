// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

pub mod bcs;
pub mod event;
pub mod hash;
pub mod signer;
pub mod string;
#[cfg(feature = "testing")]
pub mod unit_test;
pub mod vector;

#[cfg(feature = "testing")]
pub mod debug;

mod helpers;

use move_core_types::account_address::AccountAddress;
use move_vm_runtime::native_functions::{make_table_from_iter, NativeFunctionTable};

#[derive(Debug, Clone)]
pub struct GasParameters {
    pub bcs: bcs::GasParameters,
    pub event: event::GasParameters,
    pub hash: hash::GasParameters,
    pub signer: signer::GasParameters,
    pub string: string::GasParameters,
    pub vector: vector::GasParameters,

    #[cfg(feature = "testing")]
    pub unit_test: unit_test::GasParameters,
    #[cfg(feature = "testing")]
    pub debug: debug::GasParameters,
}

impl GasParameters {
    pub fn zeros() -> Self {
        Self {
            bcs: bcs::GasParameters {
                to_bytes: bcs::ToBytesGasParameters {
                    input_unit_cost: 0.into(),
                    output_unit_cost: 0.into(),
                    legacy_min_output_size: 0.into(),
                    failure_cost: 0.into(),
                },
            },

            event: event::GasParameters {
                write_to_event_store: event::WriteToEventStoreGasParameters { unit_cost: 0 },
            },

            hash: hash::GasParameters {
                sha2_256: hash::Sha2_256GasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                    legacy_min_input_len: 0.into(),
                },
                sha3_256: hash::Sha3_256GasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                    legacy_min_input_len: 0.into(),
                },
            },
            signer: signer::GasParameters {
                borrow_address: signer::BorrowAddressGasParameters {
                    base_cost: 0.into(),
                },
            },
            string: string::GasParameters {
                check_utf8: string::CheckUtf8GasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                },
                is_char_boundary: string::IsCharBoundaryGasParameters {
                    base_cost: 0.into(),
                },
                sub_string: string::SubStringGasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                },
                index_of: string::IndexOfGasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                },
            },
            vector: vector::GasParameters {
                empty: vector::EmptyGasParameters {
                    base_cost: 0.into(),
                },
                length: vector::LengthGasParameters {
                    base_cost: 0.into(),
                },
                push_back: vector::PushBackGasParameters {
                    base_cost: 0.into(),
                    legacy_unit_cost: 0.into(),
                },
                borrow: vector::BorrowGasParameters {
                    base_cost: 0.into(),
                },
                pop_back: vector::PopBackGasParameters {
                    base_cost: 0.into(),
                },
                destroy_empty: vector::DestroyEmptyGasParameters {
                    base_cost: 0.into(),
                },
                swap: vector::SwapGasParameters {
                    base_cost: 0.into(),
                },
                append: vector::AppendGasParameters {
                    base_cost: 0,
                    legacy_unit_cost: 0,
                },
                remove: vector::RemoveGasParameters {
                    base_cost: 0,
                    legacy_unit_cost: 0,
                },
                reverse: vector::ReverseGasParameters {
                    base_cost: 0,
                    legacy_unit_cost: 0,
                },
            },
            #[cfg(feature = "testing")]
            unit_test: unit_test::GasParameters {
                create_signers_for_testing: unit_test::CreateSignersForTestingGasParameters {
                    base_cost: 0.into(),
                    unit_cost: 0.into(),
                },
            },
            #[cfg(feature = "testing")]
            debug: debug::GasParameters {
                print: debug::PrintGasParameters { base_cost: 0 },
                print_stack_trace: debug::PrintStackTraceGasParameters { base_cost: 0 },
            },
        }
    }
}

pub fn all_natives(
    move_std_addr: AccountAddress,
    gas_params: GasParameters,
) -> NativeFunctionTable {
    let mut natives = vec![];

    macro_rules! add_natives {
        ($module_name: expr, $natives: expr) => {
            natives.extend(
                $natives.map(|(func_name, func)| ($module_name.to_string(), func_name, func)),
            );
        };
    }

    add_natives!("BCS", bcs::make_all(gas_params.bcs));
    add_natives!("Hash", hash::make_all(gas_params.hash));
    add_natives!("Signer", signer::make_all(gas_params.signer));
    add_natives!("String", string::make_all(gas_params.string));
    add_natives!("Vector", vector::make_all(gas_params.vector));
    #[cfg(feature = "testing")]
    {
        add_natives!("Unit_test", unit_test::make_all(gas_params.unit_test));
    }
    make_table_from_iter(move_std_addr, natives)
}

#[derive(Debug, Clone)]
pub struct NurseryGasParameters {
    event: event::GasParameters,

    #[cfg(feature = "testing")]
    debug: debug::GasParameters,
}

impl NurseryGasParameters {
    pub fn zeros() -> Self {
        Self {
            event: event::GasParameters {
                write_to_event_store: event::WriteToEventStoreGasParameters {
                    unit_cost: 0.into(),
                },
            },
            #[cfg(feature = "testing")]
            debug: debug::GasParameters {
                print: debug::PrintGasParameters {
                    base_cost: 0.into(),
                },
                print_stack_trace: debug::PrintStackTraceGasParameters {
                    base_cost: 0.into(),
                },
            },
        }
    }
}

pub fn nursery_natives(
    move_std_addr: AccountAddress,
    gas_params: NurseryGasParameters,
) -> NativeFunctionTable {
    let mut natives = vec![];

    macro_rules! add_natives {
        ($module_name: expr, $natives: expr) => {
            natives.extend(
                $natives.map(|(func_name, func)| ($module_name.to_string(), func_name, func)),
            );
        };
    }

    add_natives!("Event", event::make_all(gas_params.event));
    #[cfg(feature = "testing")]
    {
        add_natives!("Debug", debug::make_all(gas_params.debug));
    }

    make_table_from_iter(move_std_addr, natives)
}
