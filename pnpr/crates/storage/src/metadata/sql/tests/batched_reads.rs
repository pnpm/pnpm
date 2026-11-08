use super::Db;
use crate::metadata::sql::adapter::{Connection, Param, Query, ReadSource, Row};

async fn batched_reads_preserve_parameters_empty_results_and_order(db: Db) {
    let adapter = db.adapter();
    let sql = db.sql(
        "SELECT CAST(?1 AS BIGINT) AS value, CAST(?2 AS TEXT) AS label WHERE ?1 <> 3",
        "SELECT CAST(?1 AS BIGINT) AS value, CAST(?2 AS TEXT) AS label WHERE ?1 <> 3",
        "SELECT CAST(?1 AS SIGNED) AS value, CAST(?2 AS CHAR) AS label WHERE ?1 <> 3",
    );
    tokio::task::spawn_blocking(move || {
        adapter.read(ReadSource::Primary, &mut |connection| {
            connection.begin_read()?;
            let queries: Vec<_> = (0..256)
                .map(|index| Query::new(sql, &[Param::Integer(index), Param::Text("it's ?1")]))
                .collect();
            let before = mysql_statement_count(connection, sql.contains("SIGNED"))?;
            let results = connection.query_all(&queries)?;
            if let Some(before) = before {
                let after = mysql_statement_count(connection, true)?.unwrap();
                assert_eq!(after - before, 2, "one batch and the counter query");
            }
            assert_results(results)?;
            connection.commit()
        })
    })
    .await
    .unwrap()
    .unwrap();
}

conformance! {
    batched_reads_preserve_parameters_empty_results_and_order,
}

fn assert_results(results: Vec<Vec<Row>>) -> pnpr_error::Result<()> {
    assert_eq!(results.len(), 256);
    for (index, mut rows) in results.into_iter().enumerate() {
        if index == 3 {
            assert!(rows.is_empty());
            continue;
        }
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<i64>(0)?, i64::try_from(index).unwrap());
        assert_eq!(rows[0].get::<String>(1)?, "it's ?1");
    }
    Ok(())
}

fn mysql_statement_count(
    connection: &dyn Connection,
    mysql: bool,
) -> pnpr_error::Result<Option<u64>> {
    if !mysql {
        return Ok(None);
    }
    let value = connection
        .query_one("SHOW SESSION STATUS LIKE 'Com_stmt_execute'", &[])?
        .get::<String>(1)?;
    Ok(Some(value.parse().unwrap()))
}
