//! The release a pinned toolchain file moves to when its dependencies are
//! updated to their latest versions.

use crate::{
    Channel,
    ChannelName,
    RustToolchainError,
    install::is_older,
    manifest,
};
use pnpm_config::Config;
use pnpm_network::ThrottledClient;

/// The newest release of the channel `pin` follows, named as precisely as
/// `pin`, if it is newer than `pin`.
///
/// A version (`1.90.0` or `1.90`) follows `stable`, and a dated channel
/// (`nightly-2026-01-01`) follows its channel. A channel that moves by
/// itself, such as `stable`, has nothing newer to pin and returns `None`.
pub async fn newer_release(
    config: &Config,
    client: &ThrottledClient,
    pin: &Channel,
) -> Result<Option<Channel>, RustToolchainError> {
    let Some(followed) = followed_channel(pin) else { return Ok(None) };
    if config.offline {
        return Err(RustToolchainError::OfflineUpdate { pin: pin.to_string() });
    }
    let server = manifest::dist_server(config);
    let manifest = manifest::fetch(config, client, server, &followed).await?;
    let newest = as_precise_as(pin, manifest.pinned(&followed)?, manifest.date());
    Ok(is_older(pin, &newest).then_some(newest))
}

fn followed_channel(pin: &Channel) -> Option<Channel> {
    match pin {
        Channel::Version { .. } => Some(Channel::Named { name: ChannelName::Stable, date: None }),
        Channel::Named { name, date: Some(_) } => Some(Channel::Named { name: *name, date: None }),
        Channel::Named { date: None, .. } => None,
    }
}

/// `newest`, a release of the channel `pin` follows, in the form `pin` is
/// written in. `date` is the day `newest` was published.
fn as_precise_as(pin: &Channel, newest: Channel, date: &str) -> Channel {
    match (pin, newest) {
        (Channel::Version { patch: None, .. }, Channel::Version { major, minor, .. }) => {
            Channel::Version { major, minor, patch: None }
        }
        (Channel::Named { name: ChannelName::Stable, .. }, Channel::Version { .. }) => {
            Channel::Named { name: ChannelName::Stable, date: Some(date.to_string()) }
        }
        (_, newest) => newest,
    }
}

#[cfg(test)]
mod tests;
