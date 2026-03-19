// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    delayed_values::delayed_field_id::DelayedFieldID,
    values::{DeserializationSeed, SerializationReadyValue, Value},
};
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::{
    value::{IdentifierMappingKind, MoveTypeLayout},
    vm_status::StatusCode,
};
use std::cell::RefCell;

/// An extension to (de)serialize information about function values.
///
/// The current Starcoin Move revision does not serialize function values yet, but this trait and
/// context method are kept so call sites can stay aligned with the unified Aptos-style serde API.
pub trait FunctionValueExtension {
    fn max_value_nest_depth(&self) -> Option<u64>;
}

/// An extension to (de)serializer to lookup information about delayed fields.
pub(crate) struct DelayedFieldsExtension<'a> {
    /// Number of delayed fields (de)serialized, capped.
    pub(crate) delayed_fields_count: RefCell<usize>,
    /// Optional mapping to ids/values. The mapping is used to replace ids with values at
    /// serialization time and values with ids at deserialization time. If [None], ids and values
    /// are serialized as is.
    pub(crate) mapping: Option<&'a dyn ValueToIdentifierMapping>,
}

impl DelayedFieldsExtension<'_> {
    // Temporarily limit the number of delayed fields per resource, until proper charges are
    // implemented.
    // TODO[agg_v2](clean): propagate up, so this value is controlled by the gas schedule version.
    pub(crate) const MAX_DELAYED_FIELDS_PER_RESOURCE: usize = 10;

    /// Increments delayed-field count and checks the cap.
    pub(crate) fn inc_and_check_delayed_fields_count(&self) -> PartialVMResult<()> {
        *self.delayed_fields_count.borrow_mut() += 1;
        if *self.delayed_fields_count.borrow() > Self::MAX_DELAYED_FIELDS_PER_RESOURCE {
            return Err(PartialVMError::new(StatusCode::TOO_MANY_DELAYED_FIELDS)
                .with_message("Too many Delayed fields in a single resource.".to_string()));
        }
        Ok(())
    }
}

/// A (de)serializer context for a single Move [Value], containing optional extensions.
pub struct ValueSerDeContext<'a> {
    pub(crate) delayed_fields_extension: Option<DelayedFieldsExtension<'a>>,
    pub(crate) legacy_signer: bool,
    /// Maximum allowed depth of a VM value.
    pub(crate) max_value_nested_depth: Option<u64>,
}

impl<'a> ValueSerDeContext<'a> {
    /// Default (de)serializer that disallows delayed fields.
    pub fn new(max_value_nested_depth: Option<u64>) -> Self {
        Self {
            delayed_fields_extension: None,
            legacy_signer: false,
            max_value_nested_depth,
        }
    }

    /// Serialize signer with legacy format to maintain backwards compatibility.
    pub fn with_legacy_signer(mut self) -> Self {
        self.legacy_signer = true;
        self
    }

    /// Keep API compatibility with the serde context call chains.
    pub fn with_func_args_deserialization(
        mut self,
        extension: &'a dyn FunctionValueExtension,
    ) -> Self {
        if self.max_value_nested_depth.is_none() {
            self.max_value_nested_depth = extension.max_value_nest_depth();
        }
        self
    }

    /// Returns the same context but with delayed fields disabled.
    pub(crate) fn clone_without_delayed_fields(&self) -> Self {
        Self {
            delayed_fields_extension: None,
            legacy_signer: self.legacy_signer,
            max_value_nested_depth: self.max_value_nested_depth,
        }
    }

    pub(crate) fn check_depth(&self, depth: u64) -> PartialVMResult<()> {
        if self
            .max_value_nested_depth
            .is_some_and(|max_depth| depth > max_depth)
        {
            return Err(PartialVMError::new(StatusCode::VM_MAX_VALUE_DEPTH_REACHED));
        }
        Ok(())
    }

    /// Custom (de)serializer that allows delayed values to be (de)serialized as ids.
    pub fn with_delayed_fields_serde(mut self) -> Self {
        self.delayed_fields_extension = Some(DelayedFieldsExtension {
            delayed_fields_count: RefCell::new(0),
            mapping: None,
        });
        self
    }

    /// Custom (de)serializer that replaces delayed ids with values on serialization and values
    /// with ids on deserialization.
    pub fn with_delayed_fields_replacement(
        mut self,
        mapping: &'a dyn ValueToIdentifierMapping,
    ) -> Self {
        self.delayed_fields_extension = Some(DelayedFieldsExtension {
            delayed_fields_count: RefCell::new(0),
            mapping: Some(mapping),
        });
        self
    }

    /// Serializes a [Value] based on the provided layout.
    pub fn serialize(
        self,
        value: &Value,
        layout: &MoveTypeLayout,
    ) -> PartialVMResult<Option<Vec<u8>>> {
        let value = SerializationReadyValue {
            ctx: &self,
            layout,
            value: &value.0,
            depth: 1,
        };

        match bcs::to_bytes(&value).ok() {
            Some(bytes) => Ok(Some(bytes)),
            None => {
                // Preserve legacy behavior: only surface the delayed-fields cap as an error.
                if let Some(delayed_fields_extension) = self.delayed_fields_extension {
                    if delayed_fields_extension.delayed_fields_count.into_inner()
                        > DelayedFieldsExtension::MAX_DELAYED_FIELDS_PER_RESOURCE
                    {
                        return Err(PartialVMError::new(StatusCode::TOO_MANY_DELAYED_FIELDS)
                            .with_message(
                                "Too many Delayed fields in a single resource.".to_string(),
                            ));
                    }
                }
                Ok(None)
            }
        }
    }

    /// Returns serialized size of a [Value].
    pub fn serialized_size(self, value: &Value, layout: &MoveTypeLayout) -> PartialVMResult<usize> {
        let value = SerializationReadyValue {
            ctx: &self,
            layout,
            value: &value.0,
            depth: 1,
        };
        bcs::serialized_size(&value).map_err(|e| {
            PartialVMError::new(StatusCode::VALUE_SERIALIZATION_ERROR).with_message(format!(
                "failed to compute serialized size of a value: {:?}",
                e
            ))
        })
    }

    /// Deserializes bytes into a Move [Value].
    pub fn deserialize(self, bytes: &[u8], layout: &MoveTypeLayout) -> Option<Value> {
        let seed = DeserializationSeed { ctx: &self, layout };
        bcs::from_bytes_seed(seed, bytes).ok()
    }

    /// Deserializes bytes into a Move [Value], returning the underlying error.
    pub fn deserialize_or_err(
        self,
        bytes: &[u8],
        layout: &MoveTypeLayout,
    ) -> PartialVMResult<Value> {
        let seed = DeserializationSeed { ctx: &self, layout };
        bcs::from_bytes_seed(seed, bytes).map_err(|e| {
            PartialVMError::new(StatusCode::FAILED_TO_DESERIALIZE_RESOURCE)
                .with_message(format!("deserializer error: {}", e))
        })
    }
}

pub fn deserialize_and_allow_delayed_values(
    bytes: &[u8],
    layout: &MoveTypeLayout,
) -> Option<Value> {
    ValueSerDeContext::new(None)
        .with_delayed_fields_serde()
        .deserialize(bytes, layout)
}

pub fn serialize_and_allow_delayed_values(
    value: &Value,
    layout: &MoveTypeLayout,
) -> PartialVMResult<Option<Vec<u8>>> {
    ValueSerDeContext::new(None)
        .with_delayed_fields_serde()
        .serialize(value, layout)
}

/// Allow conversion between values and identifiers (delayed values). For example,
/// this trait can be implemented to fetch a concrete Move value from the global
/// state based on the identifier stored inside a delayed value.
pub trait ValueToIdentifierMapping {
    fn value_to_identifier(
        &self,
        // We need kind to distinguish between aggregators and snapshots
        // of the same type.
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
        value: Value,
    ) -> PartialVMResult<DelayedFieldID>;

    fn identifier_to_value(
        &self,
        layout: &MoveTypeLayout,
        identifier: DelayedFieldID,
    ) -> PartialVMResult<Value>;
}

pub fn deserialize_and_replace_values_with_ids(
    bytes: &[u8],
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping,
) -> Option<Value> {
    ValueSerDeContext::new(None)
        .with_delayed_fields_replacement(mapping)
        .deserialize(bytes, layout)
}

pub fn serialize_and_replace_ids_with_values(
    value: &Value,
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping,
) -> Option<Vec<u8>> {
    ValueSerDeContext::new(None)
        .with_delayed_fields_replacement(mapping)
        .serialize(value, layout)
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use move_core_types::value::MoveStructLayout;
    use std::cell::RefCell;

    struct RecordingMapping {
        expected_kind: IdentifierMappingKind,
        expected_layout: MoveTypeLayout,
        returned_id: DelayedFieldID,
        seen_values: RefCell<Vec<u128>>,
    }

    impl ValueToIdentifierMapping for RecordingMapping {
        fn value_to_identifier(
            &self,
            kind: &IdentifierMappingKind,
            layout: &MoveTypeLayout,
            value: Value,
        ) -> PartialVMResult<DelayedFieldID> {
            assert_eq!(kind, &self.expected_kind);
            assert_eq!(layout, &self.expected_layout);
            let v = match layout {
                MoveTypeLayout::U64 => value.value_as::<u64>()? as u128,
                MoveTypeLayout::U128 => value.value_as::<u128>()?,
                _ => panic!("unexpected layout"),
            };
            self.seen_values.borrow_mut().push(v);
            Ok(self.returned_id)
        }

        fn identifier_to_value(
            &self,
            _layout: &MoveTypeLayout,
            _identifier: DelayedFieldID,
        ) -> PartialVMResult<Value> {
            unreachable!()
        }
    }

    fn nested_layout(kind: IdentifierMappingKind, inner: MoveTypeLayout) -> MoveTypeLayout {
        MoveTypeLayout::Struct(MoveStructLayout::Runtime(vec![MoveTypeLayout::Struct(
            MoveStructLayout::Runtime(vec![
                MoveTypeLayout::Native(kind, Box::new(inner.clone())),
                inner,
            ]),
        )]))
    }

    fn native_layout(kind: IdentifierMappingKind, inner: MoveTypeLayout) -> MoveTypeLayout {
        MoveTypeLayout::Native(kind, Box::new(inner))
    }

    #[test]
    fn test_allow_delayed_values_helper_matches_context() {
        let layout = native_layout(IdentifierMappingKind::Aggregator, MoveTypeLayout::U64);
        let delayed = Value::delayed_value(DelayedFieldID::new_with_width(13, 8));

        let helper_bytes = serialize_and_allow_delayed_values(&delayed, &layout)
            .unwrap()
            .expect("helper serialization should succeed");
        let ctx_bytes = ValueSerDeContext::new(None)
            .with_delayed_fields_serde()
            .serialize(&delayed, &layout)
            .unwrap()
            .expect("context serialization should succeed");
        assert_eq!(helper_bytes, ctx_bytes);

        let helper_value = deserialize_and_allow_delayed_values(&helper_bytes, &layout)
            .expect("helper deserialization should succeed");
        let ctx_value = ValueSerDeContext::new(None)
            .with_delayed_fields_serde()
            .deserialize(&helper_bytes, &layout)
            .expect("context deserialization should succeed");

        let helper_roundtrip = serialize_and_allow_delayed_values(&helper_value, &layout)
            .unwrap()
            .expect("helper roundtrip serialization should succeed");
        let ctx_roundtrip = serialize_and_allow_delayed_values(&ctx_value, &layout)
            .unwrap()
            .expect("context roundtrip serialization should succeed");
        assert_eq!(helper_roundtrip, ctx_roundtrip);
        assert_eq!(helper_roundtrip, helper_bytes);
    }

    #[test]
    fn test_replace_values_with_ids_non_nested_helper_matches_context() {
        let layout = native_layout(IdentifierMappingKind::Aggregator, MoveTypeLayout::U64);
        let id = DelayedFieldID::new_with_width(31, 8);
        let helper_mapping = RecordingMapping {
            expected_kind: IdentifierMappingKind::Aggregator,
            expected_layout: MoveTypeLayout::U64,
            returned_id: id,
            seen_values: RefCell::new(Vec::new()),
        };
        let ctx_mapping = RecordingMapping {
            expected_kind: IdentifierMappingKind::Aggregator,
            expected_layout: MoveTypeLayout::U64,
            returned_id: id,
            seen_values: RefCell::new(Vec::new()),
        };
        let input = 777u64.to_le_bytes();

        let helper_value =
            deserialize_and_replace_values_with_ids(&input, &layout, &helper_mapping)
                .expect("helper replace should succeed");
        let ctx_value = ValueSerDeContext::new(None)
            .with_delayed_fields_replacement(&ctx_mapping)
            .deserialize(&input, &layout)
            .expect("context replace should succeed");

        let helper_output = serialize_and_allow_delayed_values(&helper_value, &layout)
            .unwrap()
            .expect("helper output should serialize");
        let ctx_output = serialize_and_allow_delayed_values(&ctx_value, &layout)
            .unwrap()
            .expect("context output should serialize");
        assert_eq!(helper_output, ctx_output);
        assert_eq!(
            helper_mapping.seen_values.borrow().as_slice(),
            ctx_mapping.seen_values.borrow().as_slice()
        );
    }

    #[test]
    fn test_replace_values_with_ids_nested_helper_matches_context() {
        let cases = vec![
            (
                IdentifierMappingKind::Aggregator,
                MoveTypeLayout::U64,
                DelayedFieldID::new_with_width(42, 8),
                123u128,
                999u128,
            ),
            (
                IdentifierMappingKind::Snapshot,
                MoveTypeLayout::U128,
                DelayedFieldID::new_with_width(77, 16),
                123456u128,
                789012u128,
            ),
        ];

        for (kind, inner_layout, id, base, max) in cases {
            let layout = nested_layout(kind.clone(), inner_layout.clone());
            let helper_mapping = RecordingMapping {
                expected_kind: kind.clone(),
                expected_layout: inner_layout.clone(),
                returned_id: id,
                seen_values: RefCell::new(Vec::new()),
            };
            let ctx_mapping = RecordingMapping {
                expected_kind: kind,
                expected_layout: inner_layout.clone(),
                returned_id: id,
                seen_values: RefCell::new(Vec::new()),
            };

            let input = match inner_layout {
                MoveTypeLayout::U64 => {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&(base as u64).to_le_bytes());
                    bytes.extend_from_slice(&(max as u64).to_le_bytes());
                    bytes
                }
                MoveTypeLayout::U128 => {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&base.to_le_bytes());
                    bytes.extend_from_slice(&max.to_le_bytes());
                    bytes
                }
                _ => unreachable!(),
            };

            let helper_value =
                deserialize_and_replace_values_with_ids(&input, &layout, &helper_mapping)
                    .expect("helper replace should succeed");
            let ctx_value = ValueSerDeContext::new(None)
                .with_delayed_fields_replacement(&ctx_mapping)
                .deserialize(&input, &layout)
                .expect("context replace should succeed");

            let helper_output = serialize_and_allow_delayed_values(&helper_value, &layout)
                .unwrap()
                .expect("helper output should serialize");
            let ctx_output = serialize_and_allow_delayed_values(&ctx_value, &layout)
                .unwrap()
                .expect("context output should serialize");

            assert_eq!(helper_output, ctx_output);
            assert_eq!(
                helper_mapping.seen_values.borrow().as_slice(),
                ctx_mapping.seen_values.borrow().as_slice()
            );
        }
    }

    #[test]
    fn test_replace_values_with_ids_nested_native_u64() {
        let layout = nested_layout(IdentifierMappingKind::Aggregator, MoveTypeLayout::U64);
        let mapping = RecordingMapping {
            expected_kind: IdentifierMappingKind::Aggregator,
            expected_layout: MoveTypeLayout::U64,
            returned_id: DelayedFieldID::new_with_width(42, 8),
            seen_values: RefCell::new(Vec::new()),
        };

        let base = 123u64;
        let max = 999u64;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&base.to_le_bytes());
        bytes.extend_from_slice(&max.to_le_bytes());

        let value = deserialize_and_replace_values_with_ids(&bytes, &layout, &mapping)
            .expect("replace should succeed");
        let serialized = serialize_and_allow_delayed_values(&value, &layout)
            .unwrap()
            .expect("serialize should succeed");

        assert_eq!(mapping.seen_values.borrow().as_slice(), &[base as u128]);
        assert_eq!(
            &serialized[..8],
            &mapping.returned_id.as_u64().to_le_bytes()
        );
        assert_eq!(&serialized[8..], &max.to_le_bytes());
    }

    #[test]
    fn test_replace_values_with_ids_nested_native_u128() {
        let layout = nested_layout(IdentifierMappingKind::Snapshot, MoveTypeLayout::U128);
        let mapping = RecordingMapping {
            expected_kind: IdentifierMappingKind::Snapshot,
            expected_layout: MoveTypeLayout::U128,
            returned_id: DelayedFieldID::new_with_width(77, 16),
            seen_values: RefCell::new(Vec::new()),
        };

        let base = 123456u128;
        let max = 789012u128;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&base.to_le_bytes());
        bytes.extend_from_slice(&max.to_le_bytes());

        let value = deserialize_and_replace_values_with_ids(&bytes, &layout, &mapping)
            .expect("replace should succeed");
        let serialized = serialize_and_allow_delayed_values(&value, &layout)
            .unwrap()
            .expect("serialize should succeed");

        assert_eq!(mapping.seen_values.borrow().as_slice(), &[base]);
        assert_eq!(
            &serialized[..16],
            &(mapping.returned_id.as_u64() as u128).to_le_bytes()
        );
        assert_eq!(&serialized[16..], &max.to_le_bytes());
    }
}
