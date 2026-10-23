#![allow(deprecated)]

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use libsql::Database;
use pprof::criterion::{Output, PProfProfiler};
use tokio::runtime;

fn open_in_memory() -> Database {
    Database::open(":memory:").unwrap()
}

async fn open_local_replica() -> Option<Database> {
    let db_path = match std::env::var("DB_PATH") {
        Ok(db_path) => db_path,
        Err(_) => {
            println!(
                "The DB_PATH environment variable is not set, skipping local replica benchmarks."
            );
            return None;
        }
    };
    let url = match std::env::var("URL") {
        Ok(url) => url,
        Err(_) => {
            println!("The URL environment variable is not set, skipping local replica benchmarks.");
            return None;
        }
    };
    let auth_token = match std::env::var("AUTH_TOKEN") {
        Ok(auth_token) => auth_token,
        Err(_) => {
            println!("The AUTH_TOKEN environment variable is not set, skipping local replica benchmarks.");
            return None;
        }
    };
    Some(
        Database::open_with_remote_sync(db_path, url, auth_token, None)
            .await
            .unwrap(),
    )
}

fn bench(c: &mut Criterion) {
    let rt = runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("libsql");
    group.throughput(Throughput::Elements(1));

    let db = open_in_memory();
    let conn = db.connect().unwrap();

    group.bench_function("in-memory-select-1-unprepared", |b| {
        b.to_async(&rt).iter(|| async {
            let mut rows = conn.query("SELECT 1", ()).await.unwrap();
            let row = rows.next().await.unwrap().unwrap();
            assert_eq!(row.get::<i32>(0).unwrap(), 1);
        });
    });

    // Extremely hacky block_on
    //
    // Why do we need it?
    //
    // criterion's async bencher enters the runtime for the setup
    // but does not allow us to actually execute anything on that
    // runtime because the setup Fn doesn't get a future as the
    // return value. So one might say, why not use `rt.block_on`
    // well, tokio stops you from embedded runtimes within a runtime
    // because this can lead to bad things (deadlocks!). So that means
    // we need to find a way to run the prepare future without embedding
    // the tokio runtime.
    //
    // The solution is to be hacky! From the code when using the libsql
    // version of the api we know that there isn't actually any async work
    // done and that the future always returns right away. Using this we can
    // mock poll the future via `tokio_test::task::spawn` and extract the return
    // value without actually creating any runtime. This works for now but may
    // break in the future in weird ways.
    fn block_on<F: std::future::Future<Output = R>, R>(f: F) -> R {
        let mut task = tokio_test::task::spawn(f);

        if let std::task::Poll::Ready(r) = task.poll() {
            r
        } else {
            panic!()
        }
    }

    group.bench_function("in-memory-select-1-prepared", |b| {
        b.to_async(&rt).iter_batched(
            || block_on(conn.prepare("SELECT 1")).unwrap(),
            |stmt| async move {
                let mut rows = stmt.query(()).await.unwrap();
                let row = rows.next().await.unwrap().unwrap();
                assert_eq!(row.get::<i32>(0).unwrap(), 1);
                stmt.reset();
            },
            BatchSize::SmallInput,
        );
    });

    rt.block_on(conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", ()))
        .unwrap();
    for _ in 0..1000 {
        rt.block_on(conn.execute("INSERT INTO users (name) VALUES ('FOO')", ()))
            .unwrap();
    }

    group.bench_function("in-memory-select-star-from-users-limit-1-unprepared", |b| {
        b.to_async(&rt).iter_batched(
            || block_on(conn.prepare("SELECT * FROM users LIMIT 1")).unwrap(),
            |stmt| async move {
                let mut rows = stmt.query(()).await.unwrap();
                let row = rows.next().await.unwrap().unwrap();
                assert_eq!(row.get::<i32>(0).unwrap(), 1);
                stmt.reset();
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function(
        "in-memory-select-star-from-users-limit-100-unprepared",
        |b| {
            b.to_async(&rt).iter_batched(
                || block_on(conn.prepare("SELECT * FROM users LIMIT 100")).unwrap(),
                |stmt| async move {
                    let mut rows = stmt.query(()).await.unwrap();
                    let row = rows.next().await.unwrap().unwrap();
                    assert_eq!(row.get::<i32>(0).unwrap(), 1);
                    stmt.reset();
                },
                BatchSize::SmallInput,
            );
        },
    );

    let db = match rt.block_on(open_local_replica()) {
        Some(db) => db,
        None => return,
    };
    let conn = db.connect().unwrap();

    group.bench_function("local-replica-select-1-unprepared", |b| {
        b.to_async(&rt).iter(|| async {
            let mut rows = conn.query("SELECT 1", ()).await.unwrap();
            let row = rows.next().await.unwrap().unwrap();
            assert_eq!(row.get::<i32>(0).unwrap(), 1);
        });
    });

    group.bench_function("local-replica-select-1-prepared", |b| {
        b.to_async(&rt).iter_batched(
            || block_on(conn.prepare("SELECT 1")).unwrap(),
            |stmt| async move {
                let mut rows = stmt.query(()).await.unwrap();
                let row = rows.next().await.unwrap().unwrap();
                assert_eq!(row.get::<i32>(0).unwrap(), 1);
                stmt.reset();
            },
            BatchSize::SmallInput,
        );
    });

    rt.block_on(conn.execute("DROP TABLE IF EXISTS users", ()))
        .unwrap();

    rt.block_on(db.sync()).unwrap();

    rt.block_on(conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", ()))
        .unwrap();

    rt.block_on(db.sync()).unwrap();

    for _ in 0..1000 {
        rt.block_on(conn.execute("INSERT INTO users (name) VALUES ('FOO')", ()))
            .unwrap();
    }

    rt.block_on(db.sync()).unwrap();

    group.bench_function(
        "local-replica-select-star-from-users-limit-1-unprepared",
        |b| {
            b.to_async(&rt).iter_batched(
                || block_on(conn.prepare("SELECT * FROM users LIMIT 1")).unwrap(),
                |stmt| async move {
                    let mut rows = stmt.query(()).await.unwrap();
                    let row = rows.next().await.unwrap().unwrap();
                    assert_eq!(row.get::<i32>(0).unwrap(), 1);
                    stmt.reset();
                },
                BatchSize::SmallInput,
            );
        },
    );

    group.bench_function(
        "local-replica-select-star-from-users-limit-100-unprepared",
        |b| {
            b.to_async(&rt).iter_batched(
                || block_on(conn.prepare("SELECT * FROM users LIMIT 100")).unwrap(),
                |stmt| async move {
                    let mut rows = stmt.query(()).await.unwrap();
                    let row = rows.next().await.unwrap().unwrap();
                    assert_eq!(row.get::<i32>(0).unwrap(), 1);
                    stmt.reset();
                },
                BatchSize::SmallInput,
            );
        },
    );
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench
}
criterion_main!(benches);
