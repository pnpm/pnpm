import { PnpmError } from '@pnpm/error'

export interface NodeSpecifier {
  releaseChannel: string
  useNodeVersion: string
}

const isStableVersion = (version: string) => /^[0-9]+\.[0-9]+\.[0-9]+$/.test(version)
const STABLE_RELEASE_ERROR_HINT = 'The correct syntax for stable release is strictly X.Y.Z or release/X.Y.Z'

export function parseNodeSpecifier (specifier: string): NodeSpecifier {
  if (specifier.includes('/')) {
    const [releaseChannel, useNodeVersion] = specifier.split('/')

    if (releaseChannel === 'release') {
      if (!isStableVersion(useNodeVersion)) {
        throw new PnpmError('INVALID_NODE_VERSION', `"${specifier}" is not a valid node version`, {
          hint: STABLE_RELEASE_ERROR_HINT,
        })
      }
    } else if (!useNodeVersion.includes(releaseChannel)) {
      throw new PnpmError('MISMATCHED_RELEASE_CHANNEL', `The node version (${useNodeVersion}) must contain the release channel (${releaseChannel})`)
    }

    return { releaseChannel, useNodeVersion }
  }

  const prereleaseMatch = specifier.match(/^[0-9]+\.[0-9]+\.[0-9]+-(nightly|rc|test|v8-canary)(\..+)$/)
  if (prereleaseMatch != null) {
    return { releaseChannel: prereleaseMatch[1], useNodeVersion: specifier }
  }

  if (isStableVersion(specifier)) {
    return { releaseChannel: 'release', useNodeVersion: specifier }
  }

  let hint: string | undefined
  if (['nightly', 'rc', 'test', 'v8-canary'].includes(specifier)) {
    hint = `The correct syntax for ${specifier} release is strictly X.Y.Z-${specifier}.W`
  } else if (/^[0-9]+\.[0-9]+$/.test(specifier) || /^[0-9]+$/.test(specifier) || ['release', 'stable', 'latest'].includes(specifier)) {
    hint = STABLE_RELEASE_ERROR_HINT
  }
  throw new PnpmError('INVALID_NODE_VERSION', `"${specifier}" is not a valid node version`, { hint })
}
