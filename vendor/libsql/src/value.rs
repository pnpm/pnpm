use std::str::FromStr;

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// The possible types a column can be in libsql.
#[derive(Debug, Copy, Clone)]
pub enum ValueType {
    Integer = 1,
    Real,
    Text,
    Blob,
    Null,
}

impl FromStr for ValueType {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<ValueType, Self::Err> {
        match s {
            "TEXT" => Ok(ValueType::Text),
            "INTEGER" => Ok(ValueType::Integer),
            "BLOB" => Ok(ValueType::Blob),
            "NULL" => Ok(ValueType::Null),
            "REAL" => Ok(ValueType::Real),
            _ => Err(()),
        }
    }
}

impl Value {
    /// Returns `true` if the value is [`Null`].
    ///
    /// [`Null`]: Value::Null
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` if the value is [`Integer`].
    ///
    /// [`Integer`]: Value::Integer
    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Integer(..))
    }

    /// Returns `true` if the value is [`Real`].
    ///
    /// [`Real`]: Value::Real
    #[must_use]
    pub fn is_real(&self) -> bool {
        matches!(self, Self::Real(..))
    }

    pub fn as_real(&self) -> Option<&f64> {
        if let Self::Real(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the value is [`Text`].
    ///
    /// [`Text`]: Value::Text
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(..))
    }

    pub fn as_text(&self) -> Option<&String> {
        if let Self::Text(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_integer(&self) -> Option<&i64> {
        if let Self::Integer(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the value is [`Blob`].
    ///
    /// [`Blob`]: Value::Blob
    #[must_use]
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob(..))
    }

    pub fn as_blob(&self) -> Option<&Vec<u8>> {
        if let Self::Blob(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl From<i8> for Value {
    fn from(value: i8) -> Value {
        Value::Integer(value as i64)
    }
}

impl From<i16> for Value {
    fn from(value: i16) -> Value {
        Value::Integer(value as i64)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Value {
        Value::Integer(value as i64)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Value {
        Value::Integer(value)
    }
}

impl From<u8> for Value {
    fn from(value: u8) -> Value {
        Value::Integer(value as i64)
    }
}

impl From<u16> for Value {
    fn from(value: u16) -> Value {
        Value::Integer(value as i64)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Value {
        Value::Integer(value as i64)
    }
}

impl TryFrom<u64> for Value {
    type Error = crate::Error;

    fn try_from(value: u64) -> Result<Value> {
        if value > i64::MAX as u64 {
            Err(Error::ToSqlConversionFailure(
                "u64 is too large to fit in an i64".into(),
            ))
        } else {
            Ok(Value::Integer(value as i64))
        }
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Value {
        Value::Real(value as f64)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Value {
        Value::Real(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Value {
        Value::Text(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value::Text(value)
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Value {
        Value::Blob(value.to_owned())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Value {
        Value::Blob(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Value {
        Value::Integer(value as i64)
    }
}

impl<T> From<Option<T>> for Value
where
    T: Into<Value>,
{
    fn from(value: Option<T>) -> Self {
        match value {
            Some(inner) => inner.into(),
            None => Value::Null,
        }
    }
}

#[cfg(feature = "core")]
impl From<libsql_sys::Value> for Value {
    fn from(value: libsql_sys::Value) -> Value {
        match value.value_type().into() {
            ValueType::Null => Value::Null,
            ValueType::Integer => Value::Integer(value.int64()),
            ValueType::Real => Value::Real(value.double()),
            ValueType::Text => {
                let v = value.text();
                if v.is_null() {
                    Value::Null
                } else {
                    let v = unsafe { std::ffi::CStr::from_ptr(v as *const std::ffi::c_char) };
                    let v = v.to_str().unwrap();
                    Value::Text(v.to_owned())
                }
            }
            ValueType::Blob => {
                let (len, blob) = (value.bytes(), value.blob());

                assert!(len >= 0, "unexpected negative bytes value from sqlite3");

                let mut v = Vec::with_capacity(len as usize);
                if !blob.is_null() {
                    let slice: &[u8] =
                        unsafe { std::slice::from_raw_parts(blob as *const u8, len as usize) };
                    v.extend_from_slice(slice);
                }
                Value::Blob(v)
            }
        }
    }
}

/// A borrowed version of `Value`.
#[derive(Debug)]
pub enum ValueRef<'a> {
    Null,
    Integer(i64),
    Real(f64),
    Text(&'a [u8]),
    Blob(&'a [u8]),
}

impl ValueRef<'_> {
    pub fn data_type(&self) -> ValueType {
        match *self {
            ValueRef::Null => ValueType::Null,
            ValueRef::Integer(_) => ValueType::Integer,
            ValueRef::Real(_) => ValueType::Real,
            ValueRef::Text(_) => ValueType::Text,
            ValueRef::Blob(_) => ValueType::Blob,
        }
    }

    /// Returns `true` if the value ref is [`Null`].
    ///
    /// [`Null`]: ValueRef::Null
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns `true` if the value ref is [`Integer`].
    ///
    /// [`Integer`]: ValueRef::Integer
    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Integer(..))
    }

    pub fn as_integer(&self) -> Option<&i64> {
        if let Self::Integer(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the value ref is [`Real`].
    ///
    /// [`Real`]: ValueRef::Real
    #[must_use]
    pub fn is_real(&self) -> bool {
        matches!(self, Self::Real(..))
    }

    pub fn as_real(&self) -> Option<&f64> {
        if let Self::Real(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the value ref is [`Text`].
    ///
    /// [`Text`]: ValueRef::Text
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(..))
    }

    pub fn as_text(&self) -> Option<&[u8]> {
        if let Self::Text(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns `true` if the value ref is [`Blob`].
    ///
    /// [`Blob`]: ValueRef::Blob
    #[must_use]
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob(..))
    }

    pub fn as_blob(&self) -> Option<&[u8]> {
        if let Self::Blob(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl From<ValueRef<'_>> for Value {
    fn from(vr: ValueRef<'_>) -> Value {
        match vr {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(i) => Value::Integer(i),
            ValueRef::Real(r) => Value::Real(r),
            ValueRef::Text(s) => Value::Text(String::from_utf8_lossy(s).to_string()),
            ValueRef::Blob(b) => Value::Blob(b.to_vec()),
        }
    }
}

impl<'a> From<&'a str> for ValueRef<'a> {
    fn from(s: &str) -> ValueRef<'_> {
        ValueRef::Text(s.as_bytes())
    }
}

impl<'a> From<&'a [u8]> for ValueRef<'a> {
    fn from(s: &[u8]) -> ValueRef<'_> {
        ValueRef::Blob(s)
    }
}

impl<'a> From<&'a Value> for ValueRef<'a> {
    fn from(v: &'a Value) -> ValueRef<'a> {
        match *v {
            Value::Null => ValueRef::Null,
            Value::Integer(i) => ValueRef::Integer(i),
            Value::Real(r) => ValueRef::Real(r),
            Value::Text(ref s) => ValueRef::Text(s.as_bytes()),
            Value::Blob(ref b) => ValueRef::Blob(b),
        }
    }
}

impl<'a, T> From<Option<T>> for ValueRef<'a>
where
    T: Into<ValueRef<'a>>,
{
    #[inline]
    fn from(s: Option<T>) -> ValueRef<'a> {
        match s {
            Some(x) => x.into(),
            None => ValueRef::Null,
        }
    }
}

#[cfg(feature = "core")]
impl<'a> From<libsql_sys::Value> for ValueRef<'a> {
    fn from(value: libsql_sys::Value) -> ValueRef<'a> {
        match value.value_type().into() {
            ValueType::Null => ValueRef::Null,
            ValueType::Integer => ValueRef::Integer(value.int64()),
            ValueType::Real => ValueRef::Real(value.double()),
            ValueType::Text => {
                let v = value.text();
                if v.is_null() {
                    ValueRef::Null
                } else {
                    let v = unsafe { std::ffi::CStr::from_ptr(v as *const std::ffi::c_char) };
                    ValueRef::Text(v.to_bytes())
                }
            }
            ValueType::Blob => {
                let (len, blob) = (value.bytes(), value.blob());

                assert!(len >= 0, "unexpected negative bytes value from sqlite3");

                if len > 0 {
                    let slice: &[u8] =
                        unsafe { std::slice::from_raw_parts(blob as *const u8, len as usize) };
                    ValueRef::Blob(slice)
                } else {
                    ValueRef::Blob(&[])
                }
            }
        }
    }
}

#[cfg(feature = "core")]
impl From<libsql_sys::ValueType> for ValueType {
    fn from(other: libsql_sys::ValueType) -> Self {
        match other {
            libsql_sys::ValueType::Integer => ValueType::Integer,
            libsql_sys::ValueType::Real => ValueType::Real,
            libsql_sys::ValueType::Text => ValueType::Text,
            libsql_sys::ValueType::Blob => ValueType::Blob,
            libsql_sys::ValueType::Null => ValueType::Null,
        }
    }
}

#[cfg(feature = "replication")]
impl TryFrom<&libsql_replication::rpc::proxy::Value> for Value {
    type Error = Error;

    fn try_from(value: &libsql_replication::rpc::proxy::Value) -> Result<Self> {
        #[derive(serde::Deserialize)]
        pub enum BincodeValue {
            Null,
            Integer(i64),
            Real(f64),
            Text(String),
            Blob(Vec<u8>),
        }

        Ok(
            match bincode::deserialize::<'_, BincodeValue>(&value.data[..]).map_err(Error::from)? {
                BincodeValue::Null => Value::Null,
                BincodeValue::Integer(i) => Value::Integer(i),
                BincodeValue::Real(x) => Value::Real(x),
                BincodeValue::Text(s) => Value::Text(s),
                BincodeValue::Blob(b) => Value::Blob(b),
            },
        )
    }
}

#[cfg(feature = "serde")]
mod serde_ {
    use std::marker::PhantomData;

    use serde::de::value::SeqDeserializer;
    use serde::de::{self, EnumAccess, IntoDeserializer, VariantAccess, Visitor};
    use serde::Deserialize;
    use serde::Deserializer;

    use super::*;

    impl<'de> Deserialize<'de> for Value {
        fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Value;

                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "an integer, a float, a string, a blob, or null")
                }

                fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v))
                }

                fn visit_i32<E>(self, v: i32) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_i16<E>(self, v: i16) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_i8<E>(self, v: i8) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    if v > i64::MAX as u64 {
                        Err(serde::de::Error::invalid_value(
                            de::Unexpected::Unsigned(v),
                            &"u64 is too large to fit in an i64",
                        ))
                    } else {
                        Ok(Value::Integer(v as i64))
                    }
                }

                fn visit_u32<E>(self, v: u32) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_u16<E>(self, v: u16) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_u8<E>(self, v: u8) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }

                fn visit_f64<E>(self, v: f64) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Real(v))
                }

                fn visit_f32<E>(self, v: f32) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Real(v as f64))
                }

                fn visit_byte_buf<E>(self, v: Vec<u8>) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Blob(v))
                }

                fn visit_string<E>(self, v: String) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Text(v))
                }

                fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
                where
                    E: de::Error,
                {
                    Ok(Value::Text(v.to_string()))
                }

                fn visit_some<D>(
                    self,
                    deserializer: D,
                ) -> std::result::Result<Self::Value, D::Error>
                where
                    D: Deserializer<'de>,
                {
                    Deserialize::deserialize(deserializer)
                }

                fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Value::Null)
                }

                fn visit_bool<E>(self, v: bool) -> std::result::Result<Self::Value, E>
                where
                    E: de::Error,
                {
                    Ok(Value::Integer(v as i64))
                }
            }

            deserializer.deserialize_any(V)
        }
    }

    pub struct ValueDeserializer<E> {
        value: Value,
        _pth: PhantomData<E>,
    }

    impl<'de, E: de::Error> Deserializer<'de> for ValueDeserializer<E> {
        type Error = E;

        serde::forward_to_deserialize_any! {
            i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
                bytes byte_buf unit_struct newtype_struct seq tuple tuple_struct
                map struct identifier ignored_any
        }

        fn deserialize_unit<V>(self, visitor: V) -> std::result::Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self.value {
                Value::Null => visitor.visit_unit(),
                _ => self.deserialize_any(visitor),
            }
        }

        fn deserialize_option<V>(self, visitor: V) -> std::result::Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self.value {
                Value::Null => visitor.visit_none(),
                _ => visitor.visit_some(self),
            }
        }

        fn deserialize_bool<V>(self, visitor: V) -> std::result::Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self.value {
                Value::Integer(0) => visitor.visit_bool(false),
                Value::Integer(1) => visitor.visit_bool(true),
                _ => Err(de::Error::invalid_value(
                    de::Unexpected::Other(&format!("{:?}", self.value)),
                    &"a valid sqlite boolean representation (0 or 1)",
                )),
            }
        }

        fn deserialize_any<V>(self, visitor: V) -> std::result::Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self.value {
                Value::Null => visitor.visit_none(),
                Value::Integer(i) => visitor.visit_i64(i),
                Value::Real(x) => visitor.visit_f64(x),
                Value::Text(s) => visitor.visit_string(s),
                Value::Blob(b) => visitor.visit_seq(SeqDeserializer::new(b.into_iter())),
            }
        }

        fn deserialize_enum<V>(
            self,
            _name: &'static str,
            _variants: &'static [&'static str],
            visitor: V,
        ) -> std::result::Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            struct ValueEnumAccess(String);
            impl<'de> EnumAccess<'de> for ValueEnumAccess {
                type Error = de::value::Error;
                type Variant = ValueVariantAccess;

                fn variant_seed<V>(
                    self,
                    seed: V,
                ) -> std::result::Result<(V::Value, Self::Variant), Self::Error>
                where
                    V: de::DeserializeSeed<'de>,
                {
                    seed.deserialize(self.0.into_deserializer())
                        .map(|v| (v, ValueVariantAccess))
                }
            }

            struct ValueVariantAccess;
            impl<'de> VariantAccess<'de> for ValueVariantAccess {
                type Error = de::value::Error;

                fn unit_variant(self) -> std::result::Result<(), Self::Error> {
                    Ok(())
                }

                fn newtype_variant_seed<T>(
                    self,
                    _seed: T,
                ) -> std::result::Result<T::Value, Self::Error>
                where
                    T: de::DeserializeSeed<'de>,
                {
                    Err(de::Error::custom("newtype_variant not supported"))
                }

                fn tuple_variant<V>(
                    self,
                    _len: usize,
                    _visitor: V,
                ) -> std::result::Result<V::Value, Self::Error>
                where
                    V: Visitor<'de>,
                {
                    Err(de::Error::custom("tuple_variant not supported"))
                }

                fn struct_variant<V>(
                    self,
                    _fields: &'static [&'static str],
                    _visitor: V,
                ) -> std::result::Result<V::Value, Self::Error>
                where
                    V: Visitor<'de>,
                {
                    Err(de::Error::custom("struct_variant not supported"))
                }
            }

            match self.value {
                Value::Text(s) => visitor
                    .visit_enum(ValueEnumAccess(s))
                    .map_err(de::Error::custom),

                _ => Err(de::Error::invalid_type(
                    de::Unexpected::Other(&format!("{:?}", self.value)),
                    &"a valid sqlite enum representation",
                )),
            }
        }
    }

    impl<'de, E: de::Error> IntoDeserializer<'de, E> for Value {
        type Deserializer = ValueDeserializer<E>;

        fn into_deserializer(self) -> Self::Deserializer {
            ValueDeserializer {
                value: self,
                _pth: PhantomData,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_deserialize_value() {
            fn de<'de, T>(value: Value) -> std::result::Result<T, de::value::Error>
            where
                T: Deserialize<'de>,
            {
                T::deserialize(value.into_deserializer())
            }

            #[derive(Deserialize, Debug, PartialEq)]
            enum MyEnum {
                A,
                B,
            }

            assert_eq!(de::<MyEnum>(Value::Text("A".to_string())), Ok(MyEnum::A));
            assert_eq!(de::<()>(Value::Null), Ok(()));
            assert_eq!(de::<i64>(Value::Integer(123)), Ok(123));
            assert_eq!(de::<f64>(Value::Real(123.4)), Ok(123.4));
            assert_eq!(
                de::<String>(Value::Text("abc".to_string())),
                Ok("abc".to_string())
            );
            assert_eq!(
                de::<Vec<u8>>(Value::Blob(b"abc".to_vec())),
                Ok(b"abc".to_vec())
            );

            assert_eq!(de::<Option<()>>(Value::Null), Ok(None));
            assert_eq!(de::<Option<Vec<u8>>>(Value::Null), Ok(None));
            assert_eq!(de::<Option<i64>>(Value::Integer(123)), Ok(Some(123)));
            assert_eq!(de::<Option<f64>>(Value::Real(123.4)), Ok(Some(123.4)));

            assert!(de::<i64>(Value::Null).is_err());
            assert!(de::<Vec<u8>>(Value::Null).is_err());
            assert!(de::<f64>(Value::Blob(b"abc".to_vec())).is_err());
            assert!(de::<MyEnum>(Value::Text("C".to_string())).is_err());

            assert_eq!(de::<[u8; 2]>(Value::Blob(b"aa".to_vec())), Ok([97, 97]));
        }
    }
}
