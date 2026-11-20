//! Lenient parsing of npm registry publish timestamps.

pub use pnpm_time::parse_packument_timestamp;

#[cfg(test)]
mod tests;
