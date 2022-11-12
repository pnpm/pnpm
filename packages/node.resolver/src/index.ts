import { FetchFromRegistry } from '@pnpm/fetching-types'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

interface NodeVersion {
  version: string
  lts: false | string
}

export async function resolveNodeVersions (
  fetch: FetchFromRegistry,
  opts: {
    versionSpec?: string
    nodeMirrorBaseUrl?: string
    // whether use highest version in the version list,
    // returns one version at most when it's true.
    useHighest?: boolean
  }
): Promise<string[]> {
  const { versionSpec, nodeMirrorBaseUrl, useHighest } = opts
  const response = await fetch(`${nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'}index.json`)
  const allVersions = (await response.json()) as NodeVersion[]
  let pickedVersions: string[] = []

  if (!versionSpec) {
    if (useHighest) {
      pickedVersions = [allVersions[0].version.substring(1)]
    } else {
      pickedVersions = allVersions.map(({ version }) => version.substring(1))
    }
  } else if (versionSpec === 'latest') {
    pickedVersions = [allVersions[0].version.substring(1)]
  } else {
    const { versions, versionRange } = filterVersions(allVersions, versionSpec)
    if (useHighest) {
      const pickedVersion = semver.maxSatisfying(
        versions.map(({ version }) => version), versionRange, { includePrerelease: true, loose: true })
      if (pickedVersion) {
        pickedVersions = [pickedVersion.substring(1)]
      }
    } else {
      pickedVersions = versions.map(({ version }) => version.substring(1)).filter(version => semver.satisfies(version, versionRange, {
        includePrerelease: true,
        loose: true,
      }))
    }
  }

  return pickedVersions
}

function filterVersions (versions: NodeVersion[], versionSelector: string) {
  if (versionSelector === 'lts') {
    return {
      versions: versions.filter(({ lts }) => lts !== false),
      versionRange: '*',
    }
  }
  const vst = versionSelectorType(versionSelector)
  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()
    return {
      versions: versions.filter(({ lts }) => typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion),
      versionRange: '*',
    }
  }
  return { versions, versionRange: versionSelector }
}
