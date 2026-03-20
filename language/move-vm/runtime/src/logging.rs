// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use move_binary_format::errors::{PartialVMError, VMError};
use move_core_types::vm_status::{StatusCode, StatusType};
//
// Utility functions
//

pub fn expect_no_verification_errors(err: VMError) -> VMError {
    expect_no_verification_errors_unless_bogus_storage(err)
}

pub fn expect_no_verification_errors_unless_bogus_storage(err: VMError) -> VMError {
    match err.major_status() {
        StatusCode::FUNCTION_RESOLUTION_FAILURE
        | StatusCode::NUMBER_OF_ARGUMENTS_MISMATCH
        | StatusCode::FAILED_TO_DESERIALIZE_ARGUMENT
        | StatusCode::MISSING_DEPENDENCY
        | StatusCode::UNKNOWN_BINARY_ERROR
        | StatusCode::UNKNOWN_VALIDATION_STATUS
        | StatusCode::INVALID_SIGNATURE
        | StatusCode::UNKNOWN_VERIFICATION_ERROR
        | StatusCode::UNKNOWN_INVARIANT_VIOLATION_ERROR
        | StatusCode::UNKNOWN_RUNTIME_STATUS
        | StatusCode::UNKNOWN_STATUS => return err,
        _ => {}
    }

    match err.status_type() {
        status_type @ StatusType::Deserialization | status_type @ StatusType::Verification => {
            let message = format!(
                "Unexpected verifier/deserialization error! This likely means there is code \
                stored on chain that is unverifiable!\nError: {:?}",
                &err
            );
            let (
                _old_status,
                _old_sub_status,
                _old_message,
                _stacktrace,
                location,
                indices,
                offsets,
            ) = err.all_data();
            let major_status = match status_type {
                StatusType::Deserialization => StatusCode::UNEXPECTED_DESERIALIZATION_ERROR,
                StatusType::Verification => StatusCode::UNEXPECTED_VERIFIER_ERROR,
                _ => unreachable!(),
            };

            PartialVMError::new(major_status)
                .with_message(message)
                .at_indices(indices)
                .at_code_offsets(offsets)
                .finish(location)
        }
        _ => err,
    }
}
