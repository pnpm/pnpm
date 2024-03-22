import semver from 'semver'

import versionSelectorType from 'version-selector-type'

import type { FetchFromRegistry, NodeVersion } from '@pnpm/types'

const SEMVER_OPTS = {
  includePrerelease: true,
  loose: true,
}

export async function resolveNodeVersion(
  fetch: FetchFromRegistry,
  versionSpec: string | undefined,
  nodeMirrorBaseUrl?: string | undefined
): Promise<string | null> {
  const allVersions = await fetchAllVersions(fetch, nodeMirrorBaseUrl)

  if (versionSpec === 'latest') {
    return allVersions[0]?.version ?? null
  }

  const { versions, versionRange } = filterVersions(allVersions, versionSpec)

  return semver.maxSatisfying(versions, versionRange ?? '', SEMVER_OPTS) ?? null
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
    return typeof allVersions[0]?.version === 'string' ? [allVersions[0].version] : []
  }

  const { versions, versionRange } = filterVersions(allVersions, versionSpec)

  return versions.filter((version: string): boolean => {
    return semver.satisfies(version, versionRange ?? '', SEMVER_OPTS);
  })
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

function filterVersions(versions: NodeVersion[], versionSelector: string | undefined): {
  versions: string[];
  versionRange: string | undefined;
} {
  if (versionSelector === 'lts') {
    return {
      versions: versions
        .filter(({ lts }: NodeVersion): boolean => {
          return lts !== false;
        })
        .map(({ version }: NodeVersion): string => {
          return version;
        }),
      versionRange: '*',
    }
  }

  const vst = versionSelectorType(versionSelector ?? '')

  if (vst?.type === 'tag') {
    const wantedLtsVersion = vst.normalized.toLowerCase()

    return {
      versions: versions
        .filter(
          ({ lts }: NodeVersion): boolean => {
            return typeof lts === 'string' && lts.toLowerCase() === wantedLtsVersion;
          }
        )
        .map(({ version }: NodeVersion): string => {
          return version;
        }),
      versionRange: '*',
    }
  }

  return {
    versions: versions.map(({ version }: NodeVersion): string => version),
    versionRange: versionSelector,
  }
}
