import { PnpmError } from '@pnpm/error'

export interface NodeSpecifier {
  releaseChannel: string
  versionSpecifier: string
}

const RELEASE_CHANNELS = ['nightly', 'rc', 'test', 'v8-canary', 'release']

const isStableVersion = (version: string): boolean => /^\d+\.\d+\.\d+$/.test(version)

export function parseNodeSpecifier (specifier: string): NodeSpecifier {
  // Handle "channel/version" format: "rc/18", "rc/18.0.0-rc.4", "release/22.0.0", "nightly/latest"
  if (specifier.includes('/')) {
    const [releaseChannel, versionSpecifier] = specifier.split('/', 2)
    if (!RELEASE_CHANNELS.includes(releaseChannel)) {
      throw new PnpmError('INVALID_NODE_RELEASE_CHANNEL', `"${releaseChannel}" is not a valid Node.js release channel`, {
        hint: `Valid release channels are: ${RELEASE_CHANNELS.join(', ')}`,
      })
    }
    return { releaseChannel, versionSpecifier }
  }

  // Exact prerelease version with a recognized release channel suffix.
  // e.g. "22.0.0-rc.4", "22.0.0-nightly20250315d765e70802", "22.0.0-v8-canary2025..."
  const prereleaseChannelMatch = specifier.match(/^\d+\.\d+\.\d+-(nightly|rc|test|v8-canary)/)
  if (prereleaseChannelMatch != null) {
    return { releaseChannel: prereleaseChannelMatch[1], versionSpecifier: specifier }
  }

  // Exact stable version: "22.0.0"
  if (isStableVersion(specifier)) {
    return { releaseChannel: 'release', versionSpecifier: specifier }
  }

  // Standalone release channel name means "latest from that channel".
  // e.g. "nightly" → latest nightly, "rc" → latest rc, "release" → latest release
  if (RELEASE_CHANNELS.includes(specifier)) {
    return { releaseChannel: specifier, versionSpecifier: 'latest' }
  }

  // Well-known version aliases on the stable release channel
  if (specifier === 'lts' || specifier === 'latest') {
    return { releaseChannel: 'release', versionSpecifier: specifier }
  }

  // Semver ranges ("18", "^18", ">=18", "18.x") and LTS codenames ("argon", "iron", "hydrogen")
  // are all passed through as versionSpecifier on the release channel.
  // Any truly invalid input will fail at resolution time with NODEJS_VERSION_NOT_FOUND.
  return { releaseChannel: 'release', versionSpecifier: specifier }
}
