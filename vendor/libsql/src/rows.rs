use crate::{Result, Value, ValueType};
use std::fmt;

/// Represents a libsql column.
pub struct Column<'stmt> {
    pub(crate) name: &'stmt str,
    pub(crate) origin_name: Option<&'stmt str>,
    pub(crate) table_name: Option<&'stmt str>,
    pub(crate) database_name: Option<&'stmt str>,
    pub(crate) decl_type: Option<&'stmt str>,
}

impl Column<'_> {
    /// Returns the name assigned to the column in the result set.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Returns the name of the column in the origin table.
    pub fn origin_name(&self) -> Option<&str> {
        self.origin_name
    }

    /// Returns the name of the origin table.
    pub fn table_name(&self) -> Option<&str> {
        self.table_name
    }

    /// Returns the name of the origin database.
    pub fn database_name(&self) -> Option<&str> {
        self.database_name
    }

    /// Returns the type of the column (`None` for expression).
    pub fn decl_type(&self) -> Option<&str> {
        self.decl_type
    }
}

#[async_trait::async_trait]
pub(crate) trait RowsInner: ColumnsInner {
    async fn next(&mut self) -> Result<Option<Row>>;
}

/// A set of rows returned from a connection.
pub struct Rows {
    inner: Box<dyn RowsInner + Send + Sync>,
}

impl Rows {
    pub(crate) fn new(inner: impl RowsInner + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }

    /// Get the next [`Row`] returning an error if it failed and
    /// `None` if there are no more rows.
    #[allow(clippy::should_implement_trait)]
    pub async fn next(&mut self) -> Result<Option<Row>> {
        self.inner.next().await
    }

    /// Get the count of columns in this set of rows.
    pub fn column_count(&self) -> i32 {
        self.inner.column_count()
    }

    /// Fetch the name of the column for the provided column index.
    pub fn column_name(&self, idx: i32) -> Option<&str> {
        self.inner.column_name(idx)
    }

    /// Fetch the column type from the provided column index.
    pub fn column_type(&self, idx: i32) -> Result<ValueType> {
        self.inner.column_type(idx)
    }

    /// Converts current [Rows] into asynchronous stream, fetching rows
    /// one by one. This stream can be further used with [futures::StreamExt]
    /// operators.
    #[cfg(feature = "stream")]
    pub fn into_stream(mut self) -> impl futures::Stream<Item = Result<Row>> {
        async_stream::try_stream! {
            while let Some(row) = self.next().await? {
                yield row
            }
        }
    }
}

impl fmt::Debug for Rows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rows").finish()
    }
}

/// A libsql row.
pub struct Row {
    pub(crate) inner: Box<dyn RowInner + Send + Sync>,
}

impl Row {
    /// Fetch the value at the provided column index and attempt to
    /// convert the value into the provided type `T`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # async fn run(row: &libsql::Row) {
    /// row.get::<u64>(0).unwrap();
    /// # }
    /// ```
    pub fn get<T>(&self, idx: i32) -> Result<T>
    where
        T: FromValue,
    {
        let val = self.inner.column_value(idx)?;
        T::from_sql(val)
    }

    /// Fetch the value at the provided column index.
    pub fn get_value(&self, idx: i32) -> Result<Value> {
        self.inner.column_value(idx)
    }

    /// Get a `&str` column at the provided index, errors out if the column
    /// is not of the `TEXT`.
    pub fn get_str(&self, idx: i32) -> Result<&str> {
        self.inner.column_str(idx)
    }

    /// Get the count of columns in this set of rows.
    pub fn column_count(&self) -> i32 {
        self.inner.column_count()
    }

    /// Fetch the name of the column at the provided index.
    pub fn column_name(&self, idx: i32) -> Option<&str> {
        self.inner.column_name(idx)
    }

    /// Fetch the column type from the provided index.
    pub fn column_type(&self, idx: i32) -> Result<ValueType> {
        self.inner.column_type(idx)
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        self.inner.fmt(f)
    }
}

/// Convert a `Value` into the implementors type.
pub trait FromValue: Sealed {
    fn from_sql(val: Value) -> Result<Self>
    where
        Self: Sized;
}

impl FromValue for crate::Value {
    fn from_sql(val: Value) -> Result<Self> {
        Ok(val)
    }
}
impl Sealed for crate::Value {}

impl FromValue for i32 {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Integer(i) => Ok(i as i32),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for i32 {}

impl FromValue for u32 {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Integer(i) => Ok(i as u32),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for u32 {}

impl FromValue for i64 {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Integer(i) => Ok(i),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for i64 {}

impl FromValue for u64 {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Integer(i) => Ok(i as u64),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for u64 {}

impl FromValue for f64 {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Real(f) => Ok(f),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for f64 {}

impl FromValue for Vec<u8> {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Blob(blob) => Ok(blob),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for Vec<u8> {}

impl<const N: usize> FromValue for [u8; N] {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Blob(blob) => blob
                .try_into()
                .map_err(|_| crate::Error::InvalidBlobSize(N)),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl<const N: usize> Sealed for [u8; N] {}

impl FromValue for String {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Text(s) => Ok(s),
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for String {}

impl FromValue for bool {
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Err(crate::Error::NullValue),
            Value::Integer(i) => match i {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(crate::Error::InvalidColumnType),
            },
            _ => unreachable!("invalid value type"),
        }
    }
}
impl Sealed for bool {}

impl<T> FromValue for Option<T>
where
    T: FromValue,
{
    fn from_sql(val: Value) -> Result<Self> {
        match val {
            Value::Null => Ok(None),
            _ => T::from_sql(val).map(Some),
        }
    }
}
impl<T> Sealed for Option<T> {}

pub(crate) trait ColumnsInner {
    fn column_name(&self, idx: i32) -> Option<&str>;
    fn column_type(&self, idx: i32) -> Result<ValueType>;
    fn column_count(&self) -> i32;
}

pub(crate) trait RowInner: ColumnsInner + fmt::Debug {
    fn column_value(&self, idx: i32) -> Result<Value>;
    fn column_str(&self, idx: i32) -> Result<&str>;
}

mod sealed {
    pub trait Sealed {}
}

use sealed::Sealed;
