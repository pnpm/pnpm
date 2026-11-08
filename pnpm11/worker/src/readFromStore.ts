import fs from 'node:fs'

import { pkgRequiresBuild, storedRequiresBuildNeedsManifestCheck } from '@pnpm/building.pkg-requires-build'
import { isError, PnpmError } from '@pnpm/error'
import {
  buildFileMapsFromIndex,
  checkPkgFilesIntegrity,
  type PackageFilesIndex,
  parseJsonBufferSync,
  takeVerifiedFileIntegrity,
  type VerifyResult,
} from '@pnpm/store.cafs'
import type { FilesMap } from '@pnpm/store.cafs-types'
import type { BundledManifest, DependencyManifest } from '@pnpm/types'

import { equalOrSemverEqual } from './equalOrSemverEqual.js'
import { getStoreIndex } from './storeCaches.js'
import type { PkgNameVersion, ReadPkgFromCafsMessage } from './types.js'

export function readPkgFromStoreIndex (message: ReadPkgFromCafsMessage): object {
  const { storeDir, filesIndexFile, verifyStoreIntegrity, expectedPkg, strictStorePkgContentCheck, frozenStore } = message
  const pkgFilesIndex = getStoreIndex(storeDir, frozenStore).get(filesIndexFile) as PackageFilesIndex | undefined
  if (!pkgFilesIndex) {
    return {
      status: 'success',
      verifiedFileIntegrity: takeVerifiedFileIntegrity(),
      value: {
        verified: false,
        pkgFilesIndex: null,
      },
    }
  }
  const warnings = checkStoredPkgMatchesExpected(pkgFilesIndex.manifest, { expectedPkg, strictStorePkgContentCheck })
  const verifyResult = verifyStoreIntegrity
    ? checkPkgFilesIntegrity(storeDir, pkgFilesIndex)
    : buildFileMapsFromIndex(storeDir, pkgFilesIndex)
  const bundledManifest = pkgFilesIndex.manifest
  const requiresBuild = resolveRequiresBuild(pkgFilesIndex.requiresBuild, bundledManifest, verifyResult)

  return {
    status: 'success',
    warnings,
    // Store verification happens here, in the worker, but the
    // install reports it from the main thread. Hand this worker's
    // share back with the answer it belongs to.
    verifiedFileIntegrity: takeVerifiedFileIntegrity(),
    value: {
      verified: verifyResult.passed,
      bundledManifest,
      files: {
        filesMap: verifyResult.filesMap,
        sideEffectsMaps: verifyResult.sideEffectsMaps,
        sideEffectsDiffs: verifyResult.sideEffectsDiffs,
        remoteSideEffectsQuarantine: verifyResult.remoteSideEffectsQuarantine,
        resolvedFrom: 'store',
        requiresBuild,
        requiresPrepare: pkgFilesIndex.requiresPrepare,
      },
    },
  }
}

/**
 * Throws when the package in the store is not the expected one, unless
 * `strictStorePkgContentCheck` is `false`, in which case the mismatch is
 * returned as a warning.
 */
function checkStoredPkgMatchesExpected (
  manifest: BundledManifest | undefined,
  opts: Pick<ReadPkgFromCafsMessage, 'expectedPkg' | 'strictStorePkgContentCheck'>
): string[] {
  const { expectedPkg } = opts
  if (!expectedPkg || !isStoredPkgMismatch(manifest, expectedPkg)) return []
  const msg = 'Package name or version mismatch found while reading from the store.'
  const hint = `This means that either the lockfile is broken or the package metadata (name and version) inside the package's package.json file doesn't match the metadata in the registry. Expected package: ${expectedPkg.name}@${expectedPkg.version}. Actual package in the store: ${manifest?.name}@${manifest?.version}.`
  if (opts.strictStorePkgContentCheck ?? true) {
    throw new PnpmError('UNEXPECTED_PKG_CONTENT_IN_STORE', msg, {
      hint: `${hint}\n\nIf you want to ignore this issue, set strictStorePkgContentCheck to false in your configuration`,
    })
  }
  return [`${msg} ${hint}`]
}

function isStoredPkgMismatch (manifest: BundledManifest | undefined, expectedPkg: PkgNameVersion): boolean {
  const nameMismatch = manifest?.name != null &&
    expectedPkg.name != null &&
    manifest.name.toLowerCase() !== expectedPkg.name.toLowerCase()
  if (nameMismatch) return true
  return manifest?.version != null &&
    expectedPkg.version != null &&
    !equalOrSemverEqual(manifest.version, expectedPkg.version)
}

/**
 * The caller refetches an unverified entry and ignores its build flag, so
 * its `package.json` is not read: a read error would fail the install
 * before the refetch.
 */
function resolveRequiresBuild (
  stored: boolean | undefined,
  bundledManifest: BundledManifest | undefined,
  { passed, filesMap }: VerifyResult
): boolean {
  const readManifest = (): DependencyManifest | undefined => passed ? readManifestFromCafs(filesMap) : undefined
  if (stored == null) return pkgRequiresBuild(bundledManifest ?? readManifest(), filesMap)
  if (!stored || !storedRequiresBuildNeedsManifestCheck(bundledManifest, filesMap)) return stored
  const manifest = readManifest()
  return manifest == null ? stored : pkgRequiresBuild(manifest, filesMap)
}

function readManifestFromCafs (filesMap: FilesMap): DependencyManifest | undefined {
  const manifestPath = filesMap.get('package.json')
  if (manifestPath == null) return undefined
  try {
    return parseJsonBufferSync(fs.readFileSync(manifestPath)) as DependencyManifest
  } catch (err: unknown) {
    if (err instanceof SyntaxError || (isError(err) && 'code' in err && err.code === 'ENOENT')) {
      return undefined
    }
    throw err
  }
}
