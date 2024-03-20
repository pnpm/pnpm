import semver from 'semver'

import versionSelectorType from 'version-selector-type'

import { type FetchFromRegistry } from '@pnpm/fetching-types'

interface NodeVersion {
  version: string
  lts: false | string
}

const SEMVER_OPTS = {
  includePrerelease: true,
  loose: true,
}

export async function resolveNodeVersion(
  fetch: FetchFromRegistry,
  versionSpec: string,
  nodeMirrorBaseUrl?: string | undefined
): Promise<string | null> {
  const allVersions = await fetchAllVersions(fetch, nodeMirrorBaseUrl)

  if (versionSpec === 'latest') {
    return allVersions[0].version
  }

  const { versions, versionRange } = filterVersions(allVersions, versionSpec)

  return semver.maxSatisfying(versions, versionRange, SEMVER_OPTS) ?? null
}

export async function resolveNodeVersions(
  fetch: FetchFromRegistry,
  versionSpec?: string | undefined,
  nodeMirrorBaseUrl?: string | undefined
): Promise<string[]> {
  const allVersions = await fetchAllVersions(fetch, nodeMirrorBaseUrl)

  if (!versionSpec) {
    return allVersions.map(({ version }) => version)
  }

  if (versionSpec === 'latest') {
    return [allVersions[0].version]
  }

  const { versions, versionRange } = filterVersions(allVersions, versionSpec)

  return versions.filter((version) => {
    return semver.satisfies(version, versionRange, SEMVER_OPTS);
  }
  )
}

async function fetchAllVersions(
  fetch: FetchFromRegistry,
  nodeMirrorBaseUrl?: string | undefined
): Promise<NodeVersion[]> {
  const response = await fetch(
    `${nodeMirrorBaseUrl ?? 'https://nodejs.org/download/release/'}index.json`
  )

  return ((await response.json()) as NodeVersion[]).map(({ version, lts }) => ({
    version: version.substring(1),
    lts,
  }))
}

function filterVersions(versions: NodeVersion[], versionSelector: string): {
  versions: string[];
  versionRange: string;
} {
  if (versionSelector === 'lts') {
    return {
      versions: versions
        .filter(({ lts }) => lts !== false)
        .map(({ version }) => version),
      versionRange: '*',
    }
  }

  const vst = versionSelectorType(versionSelector)

  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()

    return {
      versions: versions
        .filter(
          ({ lts }) =>
            typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion
        )
        .map(({ version }) => version),
      versionRange: '*',
    }
  }

  return {
    versions: versions.map(({ version }) => version),
    versionRange: versionSelector,
  }
}
