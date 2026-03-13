// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    delayed_values::delayed_field_id::{
        DelayedFieldID, ExtractUniqueIndex, ExtractWidth, TryFromMoveValue, TryIntoMoveValue,
    },
    values::{DeserializationSeed, SerializationReadyValue, Struct, Value},
};
use move_binary_format::errors::{PartialVMError, PartialVMResult};
use move_core_types::{
    value::{IdentifierMappingKind, MoveStructLayout, MoveTypeLayout},
    vm_status::StatusCode,
};
use serde::{
    de::{DeserializeSeed, Error as DeError},
    ser::Error as SerError,
    Deserializer, Serialize, Serializer,
};
use std::cell::RefCell;

/// An extension to (de)serialize information about function values.
///
/// Starcoin's current Move revision does not serialize function values yet, but this trait and the
/// context method are kept to align call sites with the unified serde APIs.
pub trait FunctionValueExtension {
    fn max_value_nest_depth(&self) -> Option<u64>;
}

pub trait CustomDeserializer {
    fn custom_deserialize<'d, D: Deserializer<'d>>(
        &self,
        deserializer: D,
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
    ) -> Result<Value, D::Error>;
}

pub trait CustomSerializer {
    fn custom_serialize<S: Serializer>(
        &self,
        serializer: S,
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
        id: DelayedFieldID,
    ) -> Result<S::Ok, S::Error>;
}

/// Custom (de)serializer which allows delayed values to be (de)serialized as
/// is. This means that when a delayed value is serialized, the deserialization
/// must construct the delayed value back.
pub struct RelaxedCustomSerDe {
    delayed_fields_count: RefCell<usize>,
}

impl RelaxedCustomSerDe {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            delayed_fields_count: RefCell::new(0),
        }
    }
}

// TODO[agg_v2](clean): propagate up, so this value is controlled by the gas schedule version.
// Temporarily limit the number of delayed fields per resource,
// until proper charges are implemented.
pub const MAX_DELAYED_FIELDS_PER_RESOURCE: usize = 10;

impl CustomDeserializer for RelaxedCustomSerDe {
    fn custom_deserialize<'d, D: Deserializer<'d>>(
        &self,
        deserializer: D,
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
    ) -> Result<Value, D::Error> {
        *self.delayed_fields_count.borrow_mut() += 1;

        let value = DeserializationSeed {
            custom_deserializer: None::<&RelaxedCustomSerDe>,
            layout,
        }
        .deserialize(deserializer)?;
        let (id, _width) =
            DelayedFieldID::try_from_move_value(layout, value, &()).map_err(|_| {
                D::Error::custom(format!(
                    "Custom deserialization failed for {:?} with layout {}",
                    kind, layout
                ))
            })?;
        Ok(Value::delayed_value(id))
    }
}

impl CustomSerializer for RelaxedCustomSerDe {
    fn custom_serialize<S: Serializer>(
        &self,
        serializer: S,
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
        id: DelayedFieldID,
    ) -> Result<S::Ok, S::Error> {
        *self.delayed_fields_count.borrow_mut() += 1;

        let value = id.try_into_move_value(layout).map_err(|_| {
            S::Error::custom(format!(
                "Custom serialization failed for {:?} with layout {}",
                kind, layout
            ))
        })?;
        SerializationReadyValue {
            custom_serializer: None::<&RelaxedCustomSerDe>,
            layout,
            value: &value.0,
            max_value_nest_depth: None,
            depth: 1,
        }
        .serialize(serializer)
    }
}

pub fn deserialize_and_allow_delayed_values(
    bytes: &[u8],
    layout: &MoveTypeLayout,
) -> Option<Value> {
    let native_deserializer = RelaxedCustomSerDe::new();
    let seed = DeserializationSeed {
        custom_deserializer: Some(&native_deserializer),
        layout,
    };
    bcs::from_bytes_seed(seed, bytes).ok().filter(|_| {
        // Should never happen, it should always fail first in serialize_and_allow_delayed_values
        // so we can treat it as regular deserialization error.
        native_deserializer.delayed_fields_count.into_inner() <= MAX_DELAYED_FIELDS_PER_RESOURCE
    })
}

pub fn serialize_and_allow_delayed_values(
    value: &Value,
    layout: &MoveTypeLayout,
) -> PartialVMResult<Option<Vec<u8>>> {
    serialize_and_allow_delayed_values_with_limit(value, layout, None)
}

fn serialize_and_allow_delayed_values_with_limit(
    value: &Value,
    layout: &MoveTypeLayout,
    max_value_nest_depth: Option<u64>,
) -> PartialVMResult<Option<Vec<u8>>> {
    let native_serializer = RelaxedCustomSerDe::new();
    let value = SerializationReadyValue {
        custom_serializer: Some(&native_serializer),
        layout,
        value: &value.0,
        max_value_nest_depth,
        depth: 1,
    };
    bcs::to_bytes(&value)
        .ok()
        .map(|v| {
            if native_serializer.delayed_fields_count.into_inner()
                <= MAX_DELAYED_FIELDS_PER_RESOURCE
            {
                Ok(v)
            } else {
                Err(PartialVMError::new(StatusCode::TOO_MANY_DELAYED_FIELDS)
                    .with_message("Too many Delayed fields in a single resource.".to_string()))
            }
        })
        .transpose()
}

/// Allow conversion between values and identifiers (delayed values). For example,
/// this trait can be implemented to fetch a concrete Move value from the global
/// state based on the identifier stored inside a delayed value.
pub trait ValueToIdentifierMapping {
    type Identifier;

    fn value_to_identifier(
        &self,
        // We need kind to distinguish between aggregators and snapshots
        // of the same type.
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
        value: Value,
    ) -> PartialVMResult<Self::Identifier>;

    fn identifier_to_value(
        &self,
        layout: &MoveTypeLayout,
        identifier: Self::Identifier,
    ) -> PartialVMResult<Value>;
}

fn struct_field_layouts(struct_layout: &MoveStructLayout) -> Vec<&MoveTypeLayout> {
    match struct_layout {
        MoveStructLayout::Runtime(fields) => fields.iter().collect(),
        MoveStructLayout::WithFields(fields) => fields.iter().map(|field| &field.layout).collect(),
        MoveStructLayout::WithTypes { fields, .. } => {
            fields.iter().map(|field| &field.layout).collect()
        }
    }
}

fn nested_native_integer_exchange_info(
    layout: &MoveTypeLayout,
) -> Option<(IdentifierMappingKind, MoveTypeLayout, u32)> {
    let outer = match layout {
        MoveTypeLayout::Struct(s) => s,
        _ => return None,
    };
    let outer_fields = struct_field_layouts(outer);
    if outer_fields.len() != 1 {
        return None;
    }

    let inner = match outer_fields[0] {
        MoveTypeLayout::Struct(s) => s,
        _ => return None,
    };
    let inner_fields = struct_field_layouts(inner);
    if inner_fields.len() != 2 {
        return None;
    }

    let (kind, inner_layout, width) = match inner_fields[0] {
        MoveTypeLayout::Native(kind, inner_layout) => match inner_layout.as_ref() {
            MoveTypeLayout::U64 => (kind.clone(), MoveTypeLayout::U64, 8),
            MoveTypeLayout::U128 => (kind.clone(), MoveTypeLayout::U128, 16),
            _ => return None,
        },
        _ => return None,
    };

    if inner_fields[1] != &inner_layout {
        return None;
    }

    match kind {
        IdentifierMappingKind::Aggregator | IdentifierMappingKind::Snapshot => {
            Some((kind, inner_layout, width))
        }
        _ => None,
    }
}

fn try_deserialize_nested_native_integer_with_exchange<
    I: From<u64> + ExtractWidth + ExtractUniqueIndex,
>(
    bytes: &[u8],
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping<Identifier = I>,
) -> Option<PartialVMResult<Value>> {
    let (kind, inner_layout, expected_width) = nested_native_integer_exchange_info(layout)?;
    let width = usize::try_from(expected_width).ok()?;
    if bytes.len() != width.saturating_mul(2) {
        return None;
    }

    let (base_value, max_value) = match inner_layout {
        MoveTypeLayout::U64 => {
            let mut base = [0u8; 8];
            base.copy_from_slice(&bytes[..8]);
            let mut max = [0u8; 8];
            max.copy_from_slice(&bytes[8..16]);
            (
                Value::u64(u64::from_le_bytes(base)),
                Value::u64(u64::from_le_bytes(max)),
            )
        }
        MoveTypeLayout::U128 => {
            let mut base = [0u8; 16];
            base.copy_from_slice(&bytes[..16]);
            let mut max = [0u8; 16];
            max.copy_from_slice(&bytes[16..32]);
            (
                Value::u128(u128::from_le_bytes(base)),
                Value::u128(u128::from_le_bytes(max)),
            )
        }
        _ => return None,
    };

    let id = match mapping.value_to_identifier(&kind, &inner_layout, base_value) {
        Ok(id) => id,
        Err(err) => return Some(Err(err)),
    };

    if id.extract_width() != expected_width {
        return Some(Err(PartialVMError::new(StatusCode::VM_EXTENSION_ERROR)
            .with_message(format!(
                "Nested native integer exchange width mismatch: expected {}, got {}",
                expected_width,
                id.extract_width()
            ))));
    }

    let delayed = Value::delayed_value(DelayedFieldID::new_with_width(
        id.extract_unique_index(),
        id.extract_width(),
    ));
    let inner = Value::struct_(Struct::pack([delayed, max_value]));
    Some(Ok(Value::struct_(Struct::pack([inner]))))
}

/// Aptos-style delayed fields extension:
/// - `None`: delayed fields are disabled.
/// - `Some { mapping: None }`: delayed values are (de)serialized as ids.
/// - `Some { mapping: Some(..) }`: delayed ids are exchanged with values.
struct DelayedFieldsExtension<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> {
    mapping: Option<&'a dyn ValueToIdentifierMapping<Identifier = I>>,
}

/// Serde context that keeps delayed-field behavior explicit at call sites.
pub struct ValueSerDeContext<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex = DelayedFieldID>
{
    delayed_fields_extension: Option<DelayedFieldsExtension<'a, I>>,
    max_value_nested_depth: Option<u64>,
}

impl<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> ValueSerDeContext<'a, I> {
    /// Default (de)serializer that disallows delayed fields.
    pub fn new(max_value_nested_depth: Option<u64>) -> Self {
        Self {
            delayed_fields_extension: None,
            max_value_nested_depth,
        }
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

    /// Allow delayed values to be (de)serialized as delayed ids.
    pub fn with_delayed_fields_serde(mut self) -> Self {
        self.delayed_fields_extension = Some(DelayedFieldsExtension { mapping: None });
        self
    }

    /// Replace delayed ids with values on serialization, and values with delayed ids on
    /// deserialization.
    pub fn with_delayed_fields_replacement(
        mut self,
        mapping: &'a dyn ValueToIdentifierMapping<Identifier = I>,
    ) -> Self {
        self.delayed_fields_extension = Some(DelayedFieldsExtension {
            mapping: Some(mapping),
        });
        self
    }

    pub fn serialize(
        self,
        value: &Value,
        layout: &MoveTypeLayout,
    ) -> PartialVMResult<Option<Vec<u8>>> {
        match self.delayed_fields_extension {
            None => {
                let ready = SerializationReadyValue {
                    custom_serializer: None::<&RelaxedCustomSerDe>,
                    layout,
                    value: &value.0,
                    max_value_nest_depth: self.max_value_nested_depth,
                    depth: 1,
                };
                Ok(bcs::to_bytes(&ready).ok())
            }
            Some(DelayedFieldsExtension { mapping: None }) => {
                serialize_and_allow_delayed_values_with_limit(
                    value,
                    layout,
                    self.max_value_nested_depth,
                )
            }
            Some(DelayedFieldsExtension {
                mapping: Some(mapping),
            }) => Ok(serialize_and_replace_ids_with_values_with_limit(
                value,
                layout,
                mapping,
                self.max_value_nested_depth,
            )),
        }
    }

    pub fn deserialize(self, bytes: &[u8], layout: &MoveTypeLayout) -> Option<Value> {
        let _ = self.max_value_nested_depth;
        match self.delayed_fields_extension {
            None => {
                let seed = DeserializationSeed {
                    custom_deserializer: None::<&RelaxedCustomSerDe>,
                    layout,
                };
                bcs::from_bytes_seed(seed, bytes).ok()
            }
            Some(DelayedFieldsExtension { mapping: None }) => {
                deserialize_and_allow_delayed_values(bytes, layout)
            }
            Some(DelayedFieldsExtension {
                mapping: Some(mapping),
            }) => deserialize_and_replace_values_with_ids(bytes, layout, mapping),
        }
    }
}

/// Custom (de)serializer such that:
///   1. when encountering a delayed value, ir uses its id to replace it with a concrete
///      value instance and serialize it instead;
///   2. when deserializing, the concrete value instance is replaced with a delayed value.
pub struct CustomSerDeWithExchange<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> {
    mapping: &'a dyn ValueToIdentifierMapping<Identifier = I>,
    delayed_fields_count: RefCell<usize>,
}

impl<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> CustomSerDeWithExchange<'a, I> {
    pub fn new(mapping: &'a dyn ValueToIdentifierMapping<Identifier = I>) -> Self {
        Self {
            mapping,
            delayed_fields_count: RefCell::new(0),
        }
    }
}

impl<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> CustomSerializer
    for CustomSerDeWithExchange<'a, I>
{
    fn custom_serialize<S: Serializer>(
        &self,
        serializer: S,
        _kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
        sized_id: DelayedFieldID,
    ) -> Result<S::Ok, S::Error> {
        *self.delayed_fields_count.borrow_mut() += 1;

        let value = self
            .mapping
            .identifier_to_value(layout, sized_id.as_u64().into())
            .map_err(|e| S::Error::custom(format!("{}", e)))?;
        SerializationReadyValue {
            custom_serializer: None::<&RelaxedCustomSerDe>,
            layout,
            value: &value.0,
            max_value_nest_depth: None,
            depth: 1,
        }
        .serialize(serializer)
    }
}

impl<'a, I: From<u64> + ExtractWidth + ExtractUniqueIndex> CustomDeserializer
    for CustomSerDeWithExchange<'a, I>
{
    fn custom_deserialize<'d, D: Deserializer<'d>>(
        &self,
        deserializer: D,
        kind: &IdentifierMappingKind,
        layout: &MoveTypeLayout,
    ) -> Result<Value, D::Error> {
        *self.delayed_fields_count.borrow_mut() += 1;

        let value = DeserializationSeed {
            custom_deserializer: None::<&RelaxedCustomSerDe>,
            layout,
        }
        .deserialize(deserializer)?;
        let id = self
            .mapping
            .value_to_identifier(kind, layout, value)
            .map_err(|e| D::Error::custom(format!("{}", e)))?;
        Ok(Value::delayed_value(DelayedFieldID::new_with_width(
            id.extract_unique_index(),
            id.extract_width(),
        )))
    }
}

pub fn deserialize_and_replace_values_with_ids<I: From<u64> + ExtractWidth + ExtractUniqueIndex>(
    bytes: &[u8],
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping<Identifier = I>,
) -> Option<Value> {
    if let Some(result) =
        try_deserialize_nested_native_integer_with_exchange(bytes, layout, mapping)
    {
        return result.ok();
    }

    let custom_deserializer = CustomSerDeWithExchange::new(mapping);
    let seed = DeserializationSeed {
        custom_deserializer: Some(&custom_deserializer),
        layout,
    };
    bcs::from_bytes_seed(seed, bytes).ok().filter(|_| {
        // Should never happen, it should always fail first in serialize_and_allow_delayed_values
        // so we can treat it as regular deserialization error.
        custom_deserializer.delayed_fields_count.into_inner() <= MAX_DELAYED_FIELDS_PER_RESOURCE
    })
}

pub fn serialize_and_replace_ids_with_values<I: From<u64> + ExtractWidth + ExtractUniqueIndex>(
    value: &Value,
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping<Identifier = I>,
) -> Option<Vec<u8>> {
    serialize_and_replace_ids_with_values_with_limit(value, layout, mapping, None)
}

fn serialize_and_replace_ids_with_values_with_limit<
    I: From<u64> + ExtractWidth + ExtractUniqueIndex,
>(
    value: &Value,
    layout: &MoveTypeLayout,
    mapping: &dyn ValueToIdentifierMapping<Identifier = I>,
    max_value_nest_depth: Option<u64>,
) -> Option<Vec<u8>> {
    let custom_serializer = CustomSerDeWithExchange::new(mapping);
    let value = SerializationReadyValue {
        custom_serializer: Some(&custom_serializer),
        layout,
        value: &value.0,
        max_value_nest_depth,
        depth: 1,
    };
    bcs::to_bytes(&value).ok().filter(|_| {
        // Should never happen, it should always fail first in serialize_and_allow_delayed_values
        // so we can treat it as regular deserialization error.
        custom_serializer.delayed_fields_count.into_inner() <= MAX_DELAYED_FIELDS_PER_RESOURCE
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct RecordingMapping {
        expected_kind: IdentifierMappingKind,
        expected_layout: MoveTypeLayout,
        returned_id: DelayedFieldID,
        seen_values: RefCell<Vec<u128>>,
    }

    impl ValueToIdentifierMapping for RecordingMapping {
        type Identifier = DelayedFieldID;

        fn value_to_identifier(
            &self,
            kind: &IdentifierMappingKind,
            layout: &MoveTypeLayout,
            value: Value,
        ) -> PartialVMResult<Self::Identifier> {
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
            _identifier: Self::Identifier,
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

    #[test]
    fn replace_values_with_ids_nested_native_u64() {
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

        let value =
            deserialize_and_replace_values_with_ids::<DelayedFieldID>(&bytes, &layout, &mapping)
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
    fn replace_values_with_ids_nested_native_u128() {
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

        let value =
            deserialize_and_replace_values_with_ids::<DelayedFieldID>(&bytes, &layout, &mapping)
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
