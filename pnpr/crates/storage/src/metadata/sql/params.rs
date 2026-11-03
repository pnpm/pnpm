//! The `params!` macro, in scope in every module declared after it.

/// The parameters of a statement, each bound by reference.
macro_rules! params {
    ($($param:expr),* $(,)?) => {
        &[$($crate::metadata::sql::adapter::ToParam::to_param(&$param)),*]
            as &[$crate::metadata::sql::adapter::Param<'_>]
    };
}
