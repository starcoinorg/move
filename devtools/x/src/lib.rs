// Copyright (c) The Starcoin Core Contributors
// SPDX-License-Identifier: Apache-2.0

type Result<T> = anyhow::Result<T>;

pub mod bench;
pub mod build;
pub mod cargo;
pub mod changed_since;
pub mod check;
pub mod clippy;
pub mod config;
pub mod context;
pub mod diff_summary;
pub mod fix;
pub mod fmt;
pub mod generate_summaries;
pub mod installer;
pub mod lint;
pub mod playground;
pub mod test;
pub mod nextest;
pub mod tools;
pub mod utils;
pub mod generate_workspace_hack;
