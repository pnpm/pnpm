//! The `params!` macro, in scope in every module declared after it.

/// The parameters of a statement, each bound by reference.
macro_rules! params {
    ($($param:expr),* $(,)?) => {
        &[$($crate::metadata::sql::adapter::ToParam::to_param(&$param)),*]
            as &[$crate::metadata::sql::adapter::Param<'_>]
    };
}

/// A [`Query`](crate::metadata::sql::adapter::Query) of `sql`, with each
/// parameter a reference that outlives the query.
macro_rules! query {
    ($sql:expr $(, $param:expr)* $(,)?) => {
        $crate::metadata::sql::adapter::Query {
            sql: $sql,
            params: vec![$($crate::metadata::sql::adapter::ToParam::to_param($param)),*],
        }
    };
}
