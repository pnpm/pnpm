import fs from 'node:fs/promises'

import type { PkgResolutionId, Resolution } from '@pnpm/resolving.resolver-base'
import type { StoreController } from '@pnpm/store.controller-types'
import type { PackageManifest } from '@pnpm/types'

import { type LocalDepField, pristineSpecMapKey } from './verifyLockfileLocalPolicy.js'
import {
  hasLocalFilesystemResolution,
  hasLocalGitResolution,
  isRegistryOriginResolution,
  shapeMismatchReason,
} from './verifyLockfileShape.js'

export const PROBE_FIELDS: readonly LocalDepField[] = ['dependencies', 'optionalDependencies', 'peerDependencies']

export interface ProbeCandidate {
  baseKey: string
  name: string
  version?: string
  nonSemverVersion?: string
  resolution: Resolution
  dependencies: Set<string>
  optionalDependencies: Set<string>
  peerDependencies: Set<string>
}

export async function loadPristineCandidateManifest (
  candidate: ProbeCandidate,
  storeController: StoreController | undefined,
  lockfileDir: string | undefined
): Promise<PackageManifest | undefined> {
  if (storeController == null || !isSafeNonLocalCandidateResolution(candidate)) return undefined
  const effectiveDir = lockfileDir ?? process.cwd()
  if (candidate.nonSemverVersion == null && isRegistryOriginResolution(candidate.resolution)) {
    const fromRequester = manifestMatchesCandidate(
      candidate,
      await tryRequestPristineManifest(storeController, candidate, effectiveDir)
    )
    if (fromRequester != null) return fromRequester
  }
  return manifestMatchesCandidate(
    candidate,
    await tryFetchPristineManifest(storeController, candidate, effectiveDir)
  )
}

function isSafeNonLocalCandidateResolution (candidate: ProbeCandidate): boolean {
  if (candidate.resolution == null || typeof candidate.resolution !== 'object') return false
  if (hasLocalFilesystemResolution(candidate.resolution) || hasLocalGitResolution(candidate.resolution)) {
    return false
  }
  return shapeMismatchReason(candidate.resolution, candidate.nonSemverVersion, { skipFileKeyTargetCheck: true }) == null
}

function manifestMatchesCandidate (
  candidate: ProbeCandidate,
  manifest: PackageManifest | undefined
): PackageManifest | undefined {
  if (manifest == null) return undefined
  if (manifest.name != null && manifest.name !== candidate.name) return undefined
  if (
    candidate.nonSemverVersion == null &&
    isRegistryOriginResolution(candidate.resolution) &&
    candidate.version != null &&
    manifest.version != null &&
    manifest.version !== candidate.version
  ) {
    return undefined
  }
  return manifest
}

async function tryRequestPristineManifest (
  storeController: StoreController,
  candidate: ProbeCandidate,
  lockfileDir: string
): Promise<PackageManifest | undefined> {
  if (storeController.requestPackage == null || !candidate.version) return undefined
  try {
    const response = await storeController.requestPackage(
      { alias: candidate.name, bareSpecifier: candidate.version },
      {
        currentPkg: {
          id: candidate.baseKey as PkgResolutionId,
          name: candidate.name,
          resolution: candidate.resolution,
          version: candidate.version,
        },
        downloadPriority: 0,
        lockfileDir,
        preferredVersions: Object.create(null),
        projectDir: lockfileDir,
        skipFetch: true,
      }
    )
    const raw = response.body.unhookedManifest ?? response.body.manifest
    return raw as PackageManifest | undefined
  } catch {
    return undefined
  }
}

async function tryFetchPristineManifest (
  storeController: StoreController,
  candidate: ProbeCandidate,
  lockfileDir: string
): Promise<PackageManifest | undefined> {
  if (storeController.fetchPackage == null) return undefined
  try {
    const fetched = await storeController.fetchPackage({
      fetchRawManifest: true,
      force: false,
      lockfileDir,
      pkg: {
        id: candidate.baseKey,
        name: candidate.name,
        version: candidate.version,
        resolution: candidate.resolution,
      },
    })
    const result = await fetched.fetching()
    const pkgJsonPath = result.files.filesMap.get('package.json')
    if (pkgJsonPath != null) {
      return JSON.parse(await fs.readFile(pkgJsonPath, 'utf8')) as PackageManifest
    }
    return result.bundledManifest as PackageManifest | undefined
  } catch {
    return undefined
  }
}

export function recordPristineSpecs (
  candidate: ProbeCandidate,
  manifest: PackageManifest,
  out: Map<string, Record<string, string>>
): void {
  for (const field of PROBE_FIELDS) {
    const entries = manifest[field]
    if (entries == null) continue
    const key = pristineSpecMapKey(candidate.baseKey, field)
    const record = out.get(key) ?? {}
    for (const [alias, spec] of Object.entries(entries)) {
      if (typeof spec === 'string') record[alias] = spec
    }
    out.set(key, record)
  }
}
