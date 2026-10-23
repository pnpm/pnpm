//! This module contains all `Param` related utilities and traits.

use crate::{Error, Result, Value};

mod sealed {
    pub trait Sealed {}
}

use sealed::Sealed;

/// Converts some type into parameters that can be passed
/// to libsql.
///
/// The trait is sealed and not designed to be implemented by hand
/// but instead provides a few ways to use it.
///
/// # Passing parameters to libsql
///
/// Many functions in this library let you pass parameters to libsql. Doing this
/// lets you avoid any risk of SQL injection, and is simpler than escaping
/// things manually. These functions generally contain some paramter that generically
/// accepts some implementation this trait.
///
/// # Positional parameters
///
/// These can be supplied in a few ways:
///
/// - For heterogeneous parameter lists of 16 or less items a tuple syntax is supported
///     by doing `(1, "foo")`.
/// - For hetergeneous parameter lists of 16 or greater, the [`libsql::params!`] is supported
///     by doing `libsql::params![1, "foo"]`.
/// - For homogeneous paramter types (where they are all the same type), const arrays are
///     supported by doing `[1, 2, 3]`.
///
/// # Example (positional)
///
/// ```rust,no_run
/// # use libsql::{Connection, params};
/// # async fn run(conn: Connection) -> libsql::Result<()> {
/// let mut stmt = conn.prepare("INSERT INTO test (a, b) VALUES (?1, ?2)").await?;
///
/// // Using a tuple:
/// stmt.execute((0, "foobar")).await?;
///
/// // Using `libsql::params!`:
/// stmt.execute(params![1i32, "blah"]).await?;
///
/// // array literal — non-references
/// stmt.execute([2i32, 3i32]).await?;
///
/// // array literal — references
/// stmt.execute(["foo", "bar"]).await?;
///
/// // Slice literal, references:
/// stmt.execute([2i32, 3i32]).await?;
///
/// #    Ok(())
/// # }
/// ```
///
/// # Named paramters
///
/// - For heterogeneous parameter lists of 16 or less items a tuple syntax is supported
///     by doing `(("key1", 1), ("key2", "foo"))`.
/// - For hetergeneous parameter lists of 16 or greater, the [`libsql::params!`] is supported
///     by doing `libsql::named_params!["key1": 1, "key2": "foo"]`.
/// - For homogeneous paramter types (where they are all the same type), const arrays are
///     supported by doing `[("key1", 1), ("key2, 2), ("key3", 3)]`.
///
/// # Example (named)
///
/// ```rust,no_run
/// # use libsql::{Connection, named_params};
/// # async fn run(conn: Connection) -> libsql::Result<()> {
/// let mut stmt = conn.prepare("INSERT INTO test (a, b) VALUES (:key1, :key2)").await?;
///
/// // Using a tuple:
/// stmt.execute(((":key1", 0), (":key2", "foobar"))).await?;
///
/// // Using `libsql::named_params!`:
/// stmt.execute(named_params! {":key1": 1i32, ":key2": "blah" }).await?;
///
/// // const array:
/// stmt.execute([(":key1", 2i32), (":key2", 3i32)]).await?;
///
/// #   Ok(())
/// # }
/// ```
pub trait IntoParams: Sealed {
    // Hide this because users should not be implementing this
    // themselves. We should consider sealing this trait.
    #[doc(hidden)]
    fn into_params(self) -> Result<Params>;
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub enum Params {
    None,
    Positional(Vec<Value>),
    Named(Vec<(String, Value)>),
}

/// Convert an owned iterator into Params.
///
/// # Example
///
/// ```rust
/// # use libsql::{Connection, params_from_iter, Rows};
/// # async fn run(conn: &Connection) {
///
/// let iter = vec![1, 2, 3];
///
/// conn.query(
///     "SELECT * FROM users WHERE id IN (?1, ?2, ?3)",
///     params_from_iter(iter)
/// )
/// .await
/// .unwrap();
/// # }
/// ```
pub fn params_from_iter<I>(iter: I) -> impl IntoParams
where
    I: IntoIterator,
    I::Item: IntoValue,
{
    iter.into_iter().collect::<Vec<_>>()
}

impl Sealed for () {}
impl IntoParams for () {
    fn into_params(self) -> Result<Params> {
        Ok(Params::None)
    }
}

impl Sealed for Params {}
impl IntoParams for Params {
    fn into_params(self) -> Result<Params> {
        Ok(self)
    }
}

impl Sealed for &Params {}
impl IntoParams for &Params {
    fn into_params(self) -> Result<Params> {
        Ok(self.clone())
    }
}

impl<T: IntoValue> Sealed for Vec<T> {}
impl<T: IntoValue> IntoParams for Vec<T> {
    fn into_params(self) -> Result<Params> {
        let values = self
            .into_iter()
            .map(|i| i.into_value())
            .collect::<Result<Vec<_>>>()?;

        Ok(Params::Positional(values))
    }
}

impl<T: IntoValue> Sealed for Vec<(String, T)> {}
impl<T: IntoValue> IntoParams for Vec<(String, T)> {
    fn into_params(self) -> Result<Params> {
        let values = self
            .into_iter()
            .map(|(k, v)| Ok((k, v.into_value()?)))
            .collect::<Result<Vec<_>>>()?;

        Ok(Params::Named(values))
    }
}

impl<T: IntoValue, const N: usize> Sealed for [T; N] {}
impl<T: IntoValue, const N: usize> IntoParams for [T; N] {
    fn into_params(self) -> Result<Params> {
        self.into_iter().collect::<Vec<_>>().into_params()
    }
}

impl<T: IntoValue, const N: usize> Sealed for [(&str, T); N] {}
impl<T: IntoValue, const N: usize> IntoParams for [(&str, T); N] {
    fn into_params(self) -> Result<Params> {
        self.into_iter()
            // TODO: Pretty unfortunate that we need to allocate here when we know
            // the str is likely 'static. Maybe we should convert our param names
            // to be `Cow<'static, str>`?
            .map(|(k, v)| Ok((k.to_string(), v.into_value()?)))
            .collect::<Result<Vec<_>>>()?
            .into_params()
    }
}

impl<T: IntoValue + Clone, const N: usize> Sealed for &[T; N] {}
impl<T: IntoValue + Clone, const N: usize> IntoParams for &[T; N] {
    fn into_params(self) -> Result<Params> {
        self.iter().cloned().collect::<Vec<_>>().into_params()
    }
}

// NOTICE: heavily inspired by rusqlite
macro_rules! tuple_into_params {
    ($count:literal : $(($field:tt $ftype:ident)),* $(,)?) => {
        impl<$($ftype,)*> Sealed for ($($ftype,)*) where $($ftype: IntoValue,)* {}
        impl<$($ftype,)*> IntoParams for ($($ftype,)*) where $($ftype: IntoValue,)* {
            fn into_params(self) -> Result<Params> {
                let params = Params::Positional(vec![$(self.$field.into_value()?),*]);
                Ok(params)
            }
        }
    }
}

macro_rules! named_tuple_into_params {
    ($count:literal : $(($field:tt $ftype:ident)),* $(,)?) => {
        impl<$($ftype,)*> Sealed for ($((&str, $ftype),)*) where $($ftype: IntoValue,)* {}
        impl<$($ftype,)*> IntoParams for ($((&str, $ftype),)*) where $($ftype: IntoValue,)* {
            fn into_params(self) -> Result<Params> {
                let params = Params::Named(vec![$((self.$field.0.to_string(), self.$field.1.into_value()?)),*]);
                Ok(params)
            }
        }
    }
}

named_tuple_into_params!(2: (0 A), (1 B));
named_tuple_into_params!(3: (0 A), (1 B), (2 C));
named_tuple_into_params!(4: (0 A), (1 B), (2 C), (3 D));
named_tuple_into_params!(5: (0 A), (1 B), (2 C), (3 D), (4 E));
named_tuple_into_params!(6: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F));
named_tuple_into_params!(7: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G));
named_tuple_into_params!(8: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H));
named_tuple_into_params!(9: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I));
named_tuple_into_params!(10: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J));
named_tuple_into_params!(11: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K));
named_tuple_into_params!(12: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L));
named_tuple_into_params!(13: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M));
named_tuple_into_params!(14: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N));
named_tuple_into_params!(15: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N), (14 O));
named_tuple_into_params!(16: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N), (14 O), (15 P));

tuple_into_params!(2: (0 A), (1 B));
tuple_into_params!(3: (0 A), (1 B), (2 C));
tuple_into_params!(4: (0 A), (1 B), (2 C), (3 D));
tuple_into_params!(5: (0 A), (1 B), (2 C), (3 D), (4 E));
tuple_into_params!(6: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F));
tuple_into_params!(7: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G));
tuple_into_params!(8: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H));
tuple_into_params!(9: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I));
tuple_into_params!(10: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J));
tuple_into_params!(11: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K));
tuple_into_params!(12: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L));
tuple_into_params!(13: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M));
tuple_into_params!(14: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N));
tuple_into_params!(15: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N), (14 O));
tuple_into_params!(16: (0 A), (1 B), (2 C), (3 D), (4 E), (5 F), (6 G), (7 H), (8 I), (9 J), (10 K), (11 L), (12 M), (13 N), (14 O), (15 P));

// TODO: Should we rename this to `ToSql` which makes less sense but
// matches the error variant we have in `Error`. Or should we change the
// error variant to match this breaking the few people that currently use
// this error variant.
pub trait IntoValue {
    fn into_value(self) -> Result<Value>;
}

impl<T> IntoValue for T
where
    T: TryInto<Value>,
    T::Error: Into<crate::BoxError>,
{
    fn into_value(self) -> Result<Value> {
        self.try_into()
            .map_err(|e| Error::ToSqlConversionFailure(e.into()))
    }
}

impl IntoValue for Result<Value> {
    fn into_value(self) -> Result<Value> {
        self
    }
}

#[cfg(feature = "replication")]
impl From<Params> for libsql_replication::rpc::proxy::query::Params {
    fn from(params: Params) -> Self {
        use libsql_replication::rpc::proxy;

        match params {
            Params::None => proxy::query::Params::Positional(proxy::Positional::default()),
            Params::Positional(values) => {
                let values = values
                    .iter()
                    .map(|v| bincode::serialize(v).unwrap())
                    .map(|data| proxy::Value { data })
                    .collect::<Vec<_>>();
                proxy::query::Params::Positional(proxy::Positional { values })
            }
            Params::Named(values) => {
                let (names, values) = values
                    .into_iter()
                    .map(|(name, value)| {
                        let data = bincode::serialize(&value).unwrap();
                        let value = proxy::Value { data };
                        (name, value)
                    })
                    .unzip();

                proxy::query::Params::Named(proxy::Named { names, values })
            }
        }
    }
}

/// Construct positional params from a hetergeneous set of params types.
#[macro_export]
macro_rules! params {
    () => {
       ()
    };
    ($($value:expr),* $(,)?) => {{
        use $crate::params::IntoValue;
        [$($value.into_value()),*]

    }};
}

/// Construct named params from a hetergeneous set of params types.
#[macro_export]
macro_rules! named_params {
    () => {
        ()
    };
    ($($param_name:literal: $value:expr),* $(,)?) => {{
        use $crate::params::IntoValue;
        [$(($param_name, $value.into_value())),*]
    }};
}

#[cfg(test)]
mod tests {
    use crate::Value;

    #[test]
    fn test_serialize_array() {
        assert_eq!(
            params!([0; 16])[0].as_ref().unwrap(),
            &Value::Blob(vec![0; 16])
        );
    }
}
