import { FetchFromRegistry } from '@pnpm/fetching-types'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

interface NodeVersion {
  version: string
  lts: false | string
}

export async function resolveNodeVersion (fetch: FetchFromRegistry, versionSpec: string, nodeMirrorBaseUrl: string): Promise<string | null> {
  const response = await fetch(`${nodeMirrorBaseUrl}index.json`)
  const allVersions = (await response.json()) as NodeVersion[]
  if (versionSpec === 'latest') {
    return allVersions[0].version.substring(1)
  }
  const { versions, versionRange } = filterVersions(allVersions, versionSpec)
  const pickedVersion = semver.maxSatisfying(
    versions.map(({ version }) => version), versionRange, { includePrerelease: true, loose: true })
  if (!pickedVersion) return null
  return pickedVersion.substring(1)
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
