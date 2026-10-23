use libsql::{
    replication::{Frames, SnapshotFile},
    Builder,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db = Builder::new_local_replica("test.db")
        .http_request_callback(|r| {
            let _uri = r.uri_mut();

            // You can modify any part of the http request you would like including headers
            // and the URI.
        })
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();

    let args = std::env::args().collect::<Vec<String>>();
    if args.len() < 2 {
        println!("Usage: {} <snapshots path>", args[0]);
        return;
    }
    let snapshots_path = args.get(1).unwrap();

    loop {
        let paths = std::fs::read_dir(snapshots_path).unwrap();
        for snapshot_path in paths {
            let snapshot_path = snapshot_path.unwrap().path();
            println!(
                "Applying snapshot to local database: {}\n",
                snapshot_path.display()
            );
            let snapshot = SnapshotFile::open(&snapshot_path, None).await.unwrap();
            match db.sync_frames(Frames::Snapshot(snapshot)).await {
                Ok(n) => println!(
                    "{} applied, new commit index: {n:?}",
                    snapshot_path.display()
                ),
                Err(e) => println!(
                    "Syncing frames from {} failed: {e}",
                    snapshot_path.display()
                ),
            }
        }

        let mut rows = conn.query("SELECT * FROM sqlite_master", ()).await.unwrap();
        while let Ok(Some(row)) = rows.next().await {
            println!(
                "| {:024} | {:024} | {:024} | {:024} |",
                row.get_str(0).unwrap(),
                row.get_str(1).unwrap(),
                row.get_str(2).unwrap(),
                row.get_str(3).unwrap(),
            );
        }
        println!("Sleeping for 5 seconds ...");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
