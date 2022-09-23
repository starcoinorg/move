// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

/// Representation of metadata,

#[cfg(feature = "nostd")]
use alloc::vec::Vec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Metadata {
    /// The key identifying the type of metadata.
    pub key: Vec<u8>,
    /// The value of the metadata.
    pub value: Vec<u8>,
}
