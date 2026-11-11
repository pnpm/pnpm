//! Text holding a NUL character, which `PostgreSQL` cannot store.

use super::Db;
use crate::metadata::sql::adapter::{Param, Query, ReadSource, WriteTransaction};
use pnpr_error::RegistryError;

async fn text_with_a_nul_is_refused_on_every_database(db: Db) {
    let adapter = db.adapter();
    let sql = db.sql("SELECT ?1", "SELECT CAST(?1 AS TEXT)", "SELECT ?1");
    let nul = [Param::Text("a\0b")];
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            assert_bad_request(&transaction.execute(sql, &nul));
            transaction.commit()
        })?;
        adapter.read(ReadSource::Primary, &mut |connection| {
            assert_bad_request(&connection.query(sql, &nul).map(drop));
            assert_bad_request(
                &connection
                    .query_all(&[Query::new(sql, &nul)])
                    .map(drop),
            );
            Ok(())
        })
    })
    .await
    .unwrap()
    .unwrap();
}

fn assert_bad_request(result: &pnpr_error::Result<()>) {
    assert!(matches!(result, Err(RegistryError::BadRequest { .. })), "{result:?}");
}

conformance! {
    text_with_a_nul_is_refused_on_every_database,
}
