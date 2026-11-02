use super::{BundleRange, Integration, StoredHash, integrate};
use crate::{
    proof::{LogError, verify_inclusion},
    tiles::{TILE_HEIGHT, TilePath},
};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, HashMap};
use tlog_tiles::{
    Hash, HashReader, Tile, TileHashReader, TileReader, prove_record, record_hash, tree_hash,
};

/// A stored-hash table, as the registry keeps one in SQL.
#[derive(Default)]
struct MemoryStore(BTreeMap<u64, Hash>);

impl MemoryStore {
    fn persist(&mut self, integration: &Integration) {
        for StoredHash { index, hash } in &integration.stored_hashes {
            self.0.insert(*index, *hash);
        }
    }
}

impl HashReader for MemoryStore {
    fn read_hashes(&self, indexes: &[u64]) -> Result<Vec<Hash>, tlog_tiles::Error> {
        indexes
            .iter()
            .map(|index| {
                self.0
                    .get(index)
                    .copied()
                    .ok_or(tlog_tiles::Error::IndexesNotInTree)
            })
            .collect()
    }
}

/// RFC 6962 leaves and roots from `transparency-dev/merkle`'s `testonly`.
const LEAVES: [&str; 8] = [
    "",
    "00",
    "10",
    "2021",
    "3031",
    "40414243",
    "5051525354555657",
    "606162636465666768696a6b6c6d6e6f",
];
const ROOTS: [&str; 8] = [
    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
    "fac54203e7cc696cf0dfcb42c92a1d9dbaf70ad9e621f4bd8d98662f00e3c125",
    "aeb6bcfe274b70a14fb067a5e5578264db0fa9b51af5e0ba159158f329e06e77",
    "d37ee418976dd95753c1c73862b9398fa2a2cf9b4ff0fdfe8b30cd95209614b7",
    "4e3bbb1f7b478dcfe71fb631631519a3bca12c9aefca1612bfce4c13a86264d4",
    "76e67dadbcdf1e10e1b74ddc608abd2f98dfb16fbce75277b5232a127f2087ef",
    "ddb89be403809e325750d3d263cd78929c2942b7942a34b77e122c9594a74c8c",
    "5dc9da79a70659a9ad559cb701ded9a2ab9d823aad2f4960cfe370eff4604328",
];

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

fn hex_hash(text: &str) -> Hash {
    Hash(hex(text).try_into().unwrap())
}

#[test]
fn matches_rfc_6962_roots_for_any_batching() {
    let leaves: Vec<Vec<u8>> = LEAVES
        .iter()
        .map(|leaf| hex(leaf))
        .collect();
    for batches in [vec![8], vec![1; 8], vec![3, 5], vec![2, 1, 4, 1]] {
        let mut store = MemoryStore::default();
        let mut size = 0;
        for batch in batches {
            let entries = &leaves[size as usize..size as usize + batch];
            let integration = integrate(&store, size, entries).unwrap();
            store.persist(&integration);
            size = integration.size;
            assert_eq!(integration.root, hex_hash(ROOTS[size as usize - 1]));
        }
    }
}

#[test]
fn integrating_the_same_entries_again_gives_the_same_output() {
    let mut store = MemoryStore::default();
    let first = integrate(&store, 0, &[b"a", b"b", b"c"]).unwrap();
    store.persist(&first);
    let next = integrate(&store, 3, &[b"d", b"e"]).unwrap();
    store.persist(&next);
    assert_eq!(integrate(&store, 3, &[b"d", b"e"]).unwrap(), next);
    assert_eq!(next.root, tree_hash(5, &store).unwrap());
}

#[test]
fn integrating_nothing_keeps_the_tree() {
    let mut store = MemoryStore::default();
    let first = integrate(&store, 0, &[b"a"]).unwrap();
    store.persist(&first);
    let empty: [&[u8]; 0] = [];
    let again = integrate(&store, 1, &empty).unwrap();
    assert_eq!(again.root, first.root);
    assert_eq!(again.stored_hashes, []);
    assert_eq!(again.entry_bundles(), []);
}

#[test]
fn lists_the_entry_bundles_to_publish() {
    let integration = Integration {
        previous_size: 250,
        size: 520,
        root: Hash::default(),
        stored_hashes: Vec::new(),
    };
    assert_eq!(
        integration.entry_bundles(),
        [
            BundleRange { path: TilePath::Entries { index: 0, width: 256 }, entries: 0..256 },
            BundleRange { path: TilePath::Entries { index: 1, width: 256 }, entries: 256..512 },
            BundleRange { path: TilePath::Entries { index: 2, width: 8 }, entries: 512..520 },
        ],
    );
}

/// Serves published tiles by their C2SP path, as a client fetches them.
struct PublishedTiles(HashMap<String, Vec<u8>>);

impl TileReader for PublishedTiles {
    fn height(&self) -> u8 {
        TILE_HEIGHT
    }

    fn read_tiles(&self, tiles: &[Tile]) -> Result<Vec<Vec<u8>>, tlog_tiles::Error> {
        tiles
            .iter()
            .map(|tile| {
                let path = TilePath::of_tile(tile).ok_or(tlog_tiles::Error::InvalidTile)?;
                self.0
                    .get(&path.to_string())
                    .cloned()
                    .ok_or(tlog_tiles::Error::InvalidTile)
            })
            .collect()
    }

    fn save_tiles(&self, _tiles: &[Tile], _data: &[Vec<u8>]) {}
}

#[test]
fn published_tiles_let_a_client_prove_inclusion() {
    let entries: Vec<Vec<u8>> = (0..600_u32)
        .map(|index| format!("entry {index}").into_bytes())
        .collect();
    let mut store = MemoryStore::default();
    let mut published = HashMap::new();
    let mut size = 0;
    for batch in entries.chunks(170) {
        let integration = integrate(&store, size, batch).unwrap();
        let tiles = integration.tiles(&store).unwrap();
        store.persist(&integration);
        published.extend(
            tiles
                .into_iter()
                .map(|(path, data)| (path.to_string(), data)),
        );
        size = integration.size;
    }
    let root = tree_hash(size, &store).unwrap();
    assert!(published.contains_key("tile/0/000"));
    assert!(published.contains_key("tile/0/001"));
    assert!(published.contains_key("tile/0/002.p/88"));
    assert!(published.contains_key("tile/1/000.p/2"));
    assert_eq!(published["tile/0/000"][..32], record_hash(b"entry 0").0);

    let tiles = PublishedTiles(published);
    let reader = TileHashReader::new(size, root, &tiles);
    let head = crate::checkpoint::TreeHead { origin: "log.example".to_owned(), size, hash: root };
    for index in [0, 255, 256, 599] {
        let proof = prove_record(size, index, &reader).unwrap();
        verify_inclusion(&proof, &head, index, &entries[index as usize]).unwrap();
    }
}

/// A store that loses hashes, as a broken backend might.
struct ShortStore;

impl HashReader for ShortStore {
    fn read_hashes(&self, _indexes: &[u64]) -> Result<Vec<Hash>, tlog_tiles::Error> {
        Ok(Vec::new())
    }
}

#[test]
fn a_short_read_is_an_error_not_a_panic() {
    let error = integrate(&ShortStore, 1, &[b"b"]).unwrap_err();
    assert!(matches!(error, LogError::Tlog(_)), "{error}");
}

#[test]
fn rejects_entries_too_large_for_a_bundle() {
    let large = vec![0_u8; 65_536];
    let error = integrate(&MemoryStore::default(), 0, &[large]).unwrap_err();
    assert!(matches!(error, LogError::EntryTooLarge { size: 65_536 }), "{error}");
}
