use super::{Config, HashMap, Tool};

impl Config {
    /// Where the builds of one tool are downloaded from, as
    /// `tools.<name>.mirror` names it, without the trailing slash a
    /// caller joins onto.
    #[must_use]
    pub fn tool_mirror(&self, tool: Tool) -> Option<&str> {
        self.tools
            .get(&tool)?
            .mirror
            .as_deref()
            .map(|mirror| mirror.trim_end_matches('/'))
    }

    /// Where each line of a tool's builds is downloaded from, as
    /// `tools.<name>.channels` names them. A line it does not name is
    /// left to [`Self::tool_mirror`].
    #[must_use]
    pub fn tool_channel_mirrors(&self, tool: Tool) -> HashMap<String, String> {
        self.tools
            .get(&tool)
            .and_then(|tool| tool.channels.as_ref())
            .map(|channels| {
                channels
                    .iter()
                    .map(|(channel, mirror)| {
                        (channel.clone(), mirror.trim_end_matches('/').to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
