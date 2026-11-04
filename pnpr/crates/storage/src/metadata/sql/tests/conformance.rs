//! The `conformance!` macro, in scope in every test module declared after it.

/// Run each named test, an `async fn(Db)`, against every adapter.
macro_rules! conformance {
    ($($test:ident),* $(,)?) => {
        mod on_sqlite {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    super::$test(crate::metadata::sql::tests::Db::sqlite()).await;
                }
            )*
        }

        #[cfg(feature = "backend-libsql")]
        mod on_libsql {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    super::$test(crate::metadata::sql::tests::Db::libsql(false).await).await;
                }
            )*
        }

        #[cfg(feature = "backend-libsql")]
        mod on_libsql_replica {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $test() {
                    super::$test(crate::metadata::sql::tests::Db::libsql(true).await).await;
                }
            )*
        }
    };
}
