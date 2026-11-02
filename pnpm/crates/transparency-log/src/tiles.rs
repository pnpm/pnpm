//! The [tlog-tiles](https://c2sp.org/tlog-tiles) resource paths: hash tiles
//! at `tile/<L>/<N>[.p/<W>]` and entry bundles at
//! `tile/entries/<N>[.p/<W>]`, relative to the log's URL prefix.

use derive_more::{Display, Error};
use std::{fmt, str::FromStr};
use tlog_tiles::Tile;

/// The height of every tile: a full tile holds 2⁸ hashes or entries.
pub const TILE_HEIGHT: u8 = 8;

/// The number of hashes in a full tile, and of entries in a full bundle.
pub const TILE_WIDTH: u32 = 1 << TILE_HEIGHT;

const MAX_LEVEL: u8 = 63;
const PATH_BASE: u64 = 1000;

/// A tile resource of a tiled log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TilePath {
    /// A tile of Merkle tree hashes.
    Hashes {
        /// The tile's level; level 0 holds entry hashes.
        level: u8,
        /// The tile's index within its level.
        index: u64,
        /// The number of hashes, [`TILE_WIDTH`] for a full tile.
        width: u32,
    },
    /// A bundle of entries.
    Entries {
        /// The bundle's index.
        index: u64,
        /// The number of entries, [`TILE_WIDTH`] for a full bundle.
        width: u32,
    },
}

/// Why a tile path was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("not a tile path: {path}")]
pub struct TilePathError {
    #[error(not(source))]
    path: String,
}

impl TilePath {
    /// The path of a hash tile computed by [`tlog_tiles`], which must have
    /// height [`TILE_HEIGHT`].
    #[must_use]
    pub fn of_tile(tile: &Tile) -> Option<Self> {
        (tile.height() == TILE_HEIGHT && !tile.is_data()).then(|| TilePath::Hashes {
            level: tile.level(),
            index: tile.level_index(),
            width: tile.width(),
        })
    }

    /// The [`tlog_tiles`] tile this hash tile path names, for reading hashes
    /// through [`tlog_tiles::TileHashReader`].
    #[must_use]
    pub fn to_tile(self) -> Option<Tile> {
        match self {
            TilePath::Hashes { level, index, width } => {
                Some(Tile::new(TILE_HEIGHT, level, index, width, false))
            }
            TilePath::Entries { .. } => None,
        }
    }

    /// Whether the tile is full, and so immutable.
    #[must_use]
    pub const fn is_full(self) -> bool {
        match self {
            TilePath::Hashes { width, .. } | TilePath::Entries { width, .. } => width == TILE_WIDTH,
        }
    }
}

impl fmt::Display for TilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, index, width) = match *self {
            TilePath::Hashes { level, index, width } => (level.to_string(), index, width),
            TilePath::Entries { index, width } => ("entries".to_owned(), index, width),
        };
        write!(formatter, "tile/{kind}/{}", encode_index(index))?;
        if width != TILE_WIDTH {
            write!(formatter, ".p/{width}")?;
        }
        Ok(())
    }
}

impl FromStr for TilePath {
    type Err = TilePathError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        let error = || TilePathError { path: path.to_owned() };
        let parsed = parse_tile_path(path).ok_or_else(error)?;
        if parsed.to_string() != path {
            return Err(error());
        }
        Ok(parsed)
    }
}

fn parse_tile_path(path: &str) -> Option<TilePath> {
    let (kind, rest) = path.strip_prefix("tile/")?.split_once('/')?;
    let (index, width) = match rest.rsplit_once(".p/") {
        Some((index, width)) => (index, width.parse::<u32>().ok()?),
        None => (rest, TILE_WIDTH),
    };
    if !(1..=TILE_WIDTH).contains(&width) {
        return None;
    }
    let index = decode_index(index)?;
    if kind == "entries" {
        return Some(TilePath::Entries { index, width });
    }
    let level = kind
        .parse::<u8>()
        .ok()
        .filter(|level| *level <= MAX_LEVEL)?;
    Some(TilePath::Hashes { level, index, width })
}

fn encode_index(index: u64) -> String {
    let mut encoded = format!("{:03}", index % PATH_BASE);
    let mut rest = index / PATH_BASE;
    while rest > 0 {
        encoded = format!("x{:03}/{encoded}", rest % PATH_BASE);
        rest /= PATH_BASE;
    }
    encoded
}

fn decode_index(encoded: &str) -> Option<u64> {
    encoded
        .split('/')
        .try_fold(0_u64, |index, segment| {
            let digits = segment.strip_prefix('x').unwrap_or(segment);
            if digits.len() != 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            index
                .checked_mul(PATH_BASE)?
                .checked_add(digits.parse().ok()?)
        })
}

#[cfg(test)]
mod tests;
