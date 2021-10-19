import fetch from '@pnpm/fetch'
import semver from 'semver'
import versionSelectorType from 'version-selector-type'

interface NodeVersion {
  version: string
  lts: false | string
}

export default async function resolveNodeVersion (rawVersionSelector: string) {
  const { releaseDir, version } = parseNodeVersionSelector(rawVersionSelector)
  const response = await fetch(`https://nodejs.org/download/${releaseDir}/index.json`)
  const allVersions = (await response.json()) as NodeVersion[]
  if (version === 'latest') {
    return {
      version: allVersions[0].version.substring(1),
      releaseDir,
    }
  }
  const { versions, versionSelector } = filterVersions(allVersions, version)
  const pickedVersion = semver.maxSatisfying(versions.map(({ version }) => version), versionSelector, { includePrerelease: true, loose: true })
  if (!pickedVersion) return { version: null, releaseDir }
  return {
    version: pickedVersion.substring(1),
    releaseDir,
  }
}

function parseNodeVersionSelector (rawVersionSelector: string) {
  if (rawVersionSelector.includes('/')) {
    const [releaseDir, version] = rawVersionSelector.split('/')
    return { releaseDir, version }
  }
  const prereleaseMatch = rawVersionSelector.match(/-(nightly|rc|test|v8-canary)/)
  if (prereleaseMatch != null) {
    return { releaseDir: prereleaseMatch[1], version: rawVersionSelector }
  }
  if (['nightly', 'rc', 'test', 'release', 'v8-canary'].includes(rawVersionSelector)) {
    return { releaseDir: rawVersionSelector, version: 'latest' }
  }
  return { releaseDir: 'release', version: rawVersionSelector }
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
