//! The `params!` and `query!` macros.

/// The parameters of a statement, each bound by reference.
#[macro_export]
macro_rules! params {
    ($($param:expr),* $(,)?) => {
        &[$($crate::adapter::ToParam::to_param(&$param)),*]
            as &[$crate::adapter::Param<'_>]
    };
}

/// A [`Query`](crate::adapter::Query) of `sql`, with each parameter a
/// reference that outlives the query.
#[macro_export]
macro_rules! query {
    ($sql:expr $(, $param:expr)* $(,)?) => {
        $crate::adapter::Query {
            sql: $sql,
            params: vec![$($crate::adapter::ToParam::to_param($param)),*],
        }
    };
}
