import { FetchFromRegistry } from '@pnpm/fetch'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

interface NodeVersion {
  version: string
  lts: false | string
}

export default async function resolveNodeVersion (fetch: FetchFromRegistry, version: string, nodeMirrorBaseUrl: string): Promise<string | null> {
  const response = await fetch(`${nodeMirrorBaseUrl}index.json`)
  const allVersions = (await response.json()) as NodeVersion[]
  if (version === 'latest') {
    return allVersions[0].version.substring(1)
  }
  const { versions, versionSelector } = filterVersions(allVersions, version)
  const pickedVersion = semver.maxSatisfying(versions.map(({ version }) => version), versionSelector, { includePrerelease: true, loose: true })
  if (!pickedVersion) return null
  return pickedVersion.substring(1)
}

function filterVersions (versions: NodeVersion[], versionSelector: string) {
  if (versionSelector === 'lts') {
    return {
      versions: versions.filter(({ lts }) => lts !== false),
      versionSelector: '*',
    }
  }
  const vst = versionSelectorType(versionSelector)
  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()
    return {
      versions: versions.filter(({ lts }) => typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion),
      versionSelector: '*',
    }
  }
  return { versions, versionSelector }
}
