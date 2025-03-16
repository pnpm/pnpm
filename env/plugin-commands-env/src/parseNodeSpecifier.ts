import { PnpmError } from '@pnpm/error'

export interface NodeSpecifier {
  releaseChannel: string
  useNodeVersion: string
}

const isStableVersion = (version: string): boolean => /^\d+\.\d+\.\d+$/.test(version)
const matchPrereleaseVersion = (version: string): RegExpMatchArray | null => version.match(/^\d+\.\d+\.\d+-(nightly|rc|test|v8-canary)(\..+)$/)

const STABLE_RELEASE_ERROR_HINT = 'The correct syntax for stable release is strictly X.Y.Z or release/X.Y.Z'

export function isValidVersion (specifier: string): boolean {
  if (specifier.includes('/')) {
    const [releaseChannel, useNodeVersion] = specifier.split('/')

    if (releaseChannel === 'release') {
      return isStableVersion(useNodeVersion)
    }

    return useNodeVersion.includes(releaseChannel)
  }

  return isStableVersion(specifier) || matchPrereleaseVersion(specifier) != null
}

export function parseNodeSpecifier (specifier: string): NodeSpecifier {
  if (specifier.includes('/')) {
    const [releaseChannel, useNodeVersion] = specifier.split('/')

    if (releaseChannel === 'release') {
      if (!isStableVersion(useNodeVersion)) {
        throw new PnpmError('INVALID_NODE_VERSION', `"${specifier}" is not a valid Node.js version`, {
          hint: STABLE_RELEASE_ERROR_HINT,
        })
      }
    } else if (!useNodeVersion.includes(releaseChannel)) {
      throw new PnpmError('MISMATCHED_RELEASE_CHANNEL', `Node.js version (${useNodeVersion}) must contain the release channel (${releaseChannel})`)
    }

    return { releaseChannel, useNodeVersion }
  }

  const prereleaseMatch = matchPrereleaseVersion(specifier)
  if (prereleaseMatch != null) {
    return { releaseChannel: prereleaseMatch[1], useNodeVersion: specifier }
  }

  if (isStableVersion(specifier)) {
    return { releaseChannel: 'release', useNodeVersion: specifier }
  }

  let hint: string | undefined
  if (['nightly', 'rc', 'test', 'v8-canary'].includes(specifier)) {
    hint = `The correct syntax for ${specifier} release is strictly X.Y.Z-${specifier}.W`
  } else if (/^\d+\.\d+$/.test(specifier) || /^\d+$/.test(specifier) || ['release', 'stable', 'latest'].includes(specifier)) {
    hint = STABLE_RELEASE_ERROR_HINT
  }
  throw new PnpmError('INVALID_NODE_VERSION', `"${specifier}" is not a valid Node.js version`, { hint })
}
