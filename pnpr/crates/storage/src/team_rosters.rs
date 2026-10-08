use super::{DocumentWrite, Result, Storage, validated_record_name};

/// Reserved namespace holding the team rosters of registries whose teams
/// the team API manages, one record per registry.
const TEAM_ROSTERS_DIR: &str = ".team-rosters/v0";

impl Storage {
    pub async fn read_team_roster(&self, registry: &str) -> Result<Option<Vec<u8>>> {
        self.hosted.read_record(TEAM_ROSTERS_DIR, &team_roster_key(registry)?).await
    }

    /// Write the first roster of `registry`, reporting `false` when another
    /// writer stored one first.
    pub async fn create_team_roster(&self, registry: &str, bytes: &[u8]) -> Result<bool> {
        self.hosted.create_record(TEAM_ROSTERS_DIR, &team_roster_key(registry)?, bytes).await
    }

    /// Replace the roster of `registry` only while it still holds `expected`.
    pub async fn replace_team_roster_if_current(
        &self,
        registry: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let key = team_roster_key(registry)?;
        self.hosted.replace_record_if_current(TEAM_ROSTERS_DIR, &key, expected, bytes).await
    }
}

/// A registry's record key. A registry declared under an ecosystem group is
/// named `<ecosystem>/<name>`, and each segment becomes one path segment.
fn team_roster_key(registry: &str) -> Result<String> {
    let segments = registry
        .split('/')
        .map(validated_record_name)
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{}.json", segments.join("/")))
}
