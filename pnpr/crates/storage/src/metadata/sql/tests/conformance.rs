//! The `conformance!` macro, in scope in every test module declared after it.

/// Run each named test, an `async fn(Db)`, against every adapter.
macro_rules! embedded_conformance {
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

macro_rules! conformance {
    ($($test:ident),* $(,)?) => {
        embedded_conformance!($($test),*);
        #[cfg(feature = "backend-postgres")]
        mod on_postgres {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                #[ignore = "requires PNPR_TEST_POSTGRES_URL"]
                async fn $test() {
                    super::$test(crate::metadata::sql::tests::Db::network("PNPR_TEST_POSTGRES_URL").await).await;
                }
            )*
        }
        #[cfg(feature = "backend-mysql")]
        mod on_mysql {
            $(
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                #[ignore = "requires PNPR_TEST_MYSQL_URL"]
                async fn $test() {
                    super::$test(crate::metadata::sql::tests::Db::network("PNPR_TEST_MYSQL_URL").await).await;
                }
            )*
        }
    };
}
