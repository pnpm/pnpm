use super::{TILE_HEIGHT, TilePath};
use pretty_assertions::assert_eq;
use tlog_tiles::Tile;

#[test]
fn formats_paths_as_tlog_tiles_specifies() {
    let partial = TilePath::Hashes { level: 4, index: 1_234_067, width: 1 };
    assert_eq!(partial.to_string(), "tile/4/x001/x234/067.p/1");
    let full = TilePath::Hashes { level: 0, index: 1_234_067, width: 256 };
    assert_eq!(full.to_string(), "tile/0/x001/x234/067");
    let bundle = TilePath::Entries { index: 0, width: 17 };
    assert_eq!(bundle.to_string(), "tile/entries/000.p/17");
    let bundle = TilePath::Entries { index: 1000, width: 256 };
    assert_eq!(bundle.to_string(), "tile/entries/x001/000");
}

#[test]
fn parses_canonical_paths() {
    for path in [
        "tile/0/000",
        "tile/0/000.p/1",
        "tile/63/x001/x234/067.p/255",
        "tile/entries/x001/000",
        "tile/entries/999.p/3",
    ] {
        let parsed: TilePath = path.parse().unwrap();
        assert_eq!(parsed.to_string(), path);
    }
}

#[test]
fn rejects_non_canonical_paths() {
    for path in [
        "tile/00/000",
        "tile/0/0",
        "tile/0/x000/001",
        "tile/0/000.p/256",
        "tile/0/000.p/0",
        "tile/0/000.p/01",
        "tile/64/000",
        "tile/entries/000/",
        "tile/data/000",
        "tile/8/0/000",
        "tiles/0/000",
        "tile/0/x018/x446/x744/x073/x709/x551/x616",
    ] {
        assert!(path.parse::<TilePath>().is_err(), "accepted {path}");
    }
}

#[test]
fn lists_the_tiles_of_a_tree_of_70000_entries_as_the_spec_does() {
    let tiles: Vec<TilePath> = Tile::new_tiles(TILE_HEIGHT, 0, 70_000)
        .iter()
        .map(|tile| TilePath::of_tile(tile).unwrap())
        .collect();
    let count = |level: u8, is_full: bool| {
        tiles
            .iter()
            .filter(|path| {
                matches!(path, TilePath::Hashes { level: found, .. } if *found == level)
                    && path.is_full() == is_full
            })
            .count()
    };
    assert_eq!(count(0, true), 273);
    assert_eq!(count(0, false), 1);
    assert_eq!(count(1, true), 1);
    assert_eq!(count(1, false), 1);
    assert_eq!(count(2, false), 1);
    assert_eq!(tiles.len(), 277);
    assert!(tiles.contains(&TilePath::Hashes { level: 0, index: 273, width: 112 }));
    assert!(tiles.contains(&TilePath::Hashes { level: 1, index: 1, width: 17 }));
    assert!(tiles.contains(&TilePath::Hashes { level: 2, index: 0, width: 1 }));
}

#[test]
fn converts_hash_tiles_to_and_from_tlog_tiles() {
    let path = TilePath::Hashes { level: 1, index: 5, width: 9 };
    let tile = path.to_tile().unwrap();
    assert_eq!(TilePath::of_tile(&tile), Some(path));
    assert_eq!(TilePath::Entries { index: 5, width: 9 }.to_tile(), None);
    assert_eq!(TilePath::of_tile(&Tile::new(2, 0, 0, 1, false)), None);
}
