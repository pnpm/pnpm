#![allow(deprecated)]

use libsql::{replication::Frames, Database};
use libsql_replication::{
    frame::{FrameBorrowed, FrameHeader, FrameMut},
    LIBSQL_PAGE_SIZE,
};

const DB: &[u8] = include_bytes!("test.db");

#[tokio::test]
async fn inject_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open_with_local_sync(tmp.path().join("data").to_str().unwrap(), None)
        .await
        .unwrap();

    let mut frames: Vec<FrameMut> = DB
        .chunks(LIBSQL_PAGE_SIZE)
        .enumerate()
        .map(|(i, data)| {
            let header = FrameHeader {
                frame_no: (i as u64).into(),
                checksum: 0.into(),
                page_no: (i as u32 + 1).into(),
                size_after: 0.into(),
            };
            FrameBorrowed::from_parts(&header, data).into()
        })
        .collect();

    frames.last_mut().unwrap().header_mut().size_after = (frames.len() as u32).into();

    let frames = frames.into_iter().map(Into::into).collect();

    assert_eq!(
        db.sync_frames(Frames::Vec(frames)).await.unwrap().unwrap(),
        2
    );

    let conn = db.connect().unwrap();
    let mut rows = conn.query("select count(*) from test", ()).await.unwrap();
    assert_eq!(
        *rows
            .next()
            .await
            .unwrap()
            .unwrap()
            .get_value(0)
            .unwrap()
            .as_integer()
            .unwrap(),
        10
    );

    // inject the same frames again, this should be idempotent
    let mut frames: Vec<FrameMut> = DB
        .chunks(LIBSQL_PAGE_SIZE)
        .enumerate()
        .map(|(i, data)| {
            let header = FrameHeader {
                frame_no: (i as u64 + 3).into(),
                checksum: 0.into(),
                page_no: (i as u32 + 1).into(),
                size_after: 0.into(),
            };
            FrameBorrowed::from_parts(&header, data).into()
        })
        .collect();

    frames.last_mut().unwrap().header_mut().size_after = (frames.len() as u32).into();

    let frames = frames.into_iter().map(Into::into).collect();

    assert_eq!(
        db.sync_frames(libsql::replication::Frames::Vec(frames))
            .await
            .unwrap()
            .unwrap(),
        5
    );

    let conn = db.connect().unwrap();
    let mut rows = conn.query("select count(*) from test", ()).await.unwrap();
    assert_eq!(
        *rows
            .next()
            .await
            .unwrap()
            .unwrap()
            .get_value(0)
            .unwrap()
            .as_integer()
            .unwrap(),
        10
    );
}

#[tokio::test]
async fn inject_frames_split_txn() {
    let tmp = tempfile::tempdir().unwrap();
    let db = Database::open_with_local_sync(tmp.path().join("data").to_str().unwrap(), None)
        .await
        .unwrap();

    let mut frames = DB.chunks(LIBSQL_PAGE_SIZE).enumerate().map(|(i, data)| {
        let header = FrameHeader {
            frame_no: (i as u64).into(),
            checksum: 0.into(),
            page_no: (i as u32 + 1).into(),
            size_after: 0.into(),
        };
        FrameBorrowed::from_parts(&header, data)
    });

    let conn = db.connect().unwrap();
    assert!(conn.query("select count(*) from test", ()).await.is_err());

    assert!(db
        .sync_frames(Frames::Vec(vec![frames.next().unwrap().into()]))
        .await
        .unwrap()
        .is_none());
    assert!(db.flush_replicator().await.unwrap().is_none());
    assert!(conn.query("select count(*) from test", ()).await.is_err());

    assert!(db
        .sync_frames(Frames::Vec(vec![frames.next().unwrap().into()]))
        .await
        .unwrap()
        .is_none());
    assert!(db.flush_replicator().await.unwrap().is_none());
    assert!(conn.query("select count(*) from test", ()).await.is_err());

    // commit frame
    assert_eq!(
        db.sync_frames(Frames::Vec(vec![frames
            .next()
            .map(|mut f| {
                f.header_mut().size_after = 3.into();
                f
            })
            .unwrap()
            .into()]))
            .await
            .unwrap()
            .unwrap(),
        2
    );
    assert_eq!(db.flush_replicator().await.unwrap().unwrap(), 2);
    let mut rows = conn.query("select count(*) from test", ()).await.unwrap();
    assert_eq!(
        *rows
            .next()
            .await
            .unwrap()
            .unwrap()
            .get_value(0)
            .unwrap()
            .as_integer()
            .unwrap(),
        10
    );
}
