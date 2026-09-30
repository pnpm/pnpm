import { createReadStream, promises as fs } from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { resolvePackageBuildPermission } from '@pnpm/exec.prepare-package'
import gfs from '@pnpm/fs.graceful-fs'
import {
  type AtomicResolution,
  classifyResolution,
} from '@pnpm/resolving.resolver-base'
import {
  normalizeBundledManifest,
} from '@pnpm/store.cafs'
import type {
  BundledManifest,
  FetchPackageToStoreOptions,
} from '@pnpm/store.controller-types'
import type { DependencyManifest, DepPath } from '@pnpm/types'
import { loadJsonFile } from 'load-json-file'
import ssri from 'ssri'

import { isGitResolutionKind } from './getFilesIndexFilePath.js'

export const TARBALL_INTEGRITY_FILENAME = 'tarball-integrity'

export async function cachedPackageCanBeReused (opts: {
  allowBuild?: FetchPackageToStoreOptions['allowBuild']
  bundledManifest?: BundledManifest
  filesMap: Map<string, string>
  filesIndexFile: string
  ignoredBuild?: boolean
  ignoreScripts?: boolean
  pkgResolutionId: string
  requiresPrepare?: boolean
  resolutionKind: ReturnType<typeof classifyResolution>
}): Promise<boolean> {
  if (opts.ignoreScripts || !isGitResolutionKind(opts.resolutionKind)) return true
  if (opts.requiresPrepare === false) return true
  const pkgJsonPath = opts.filesMap.get('package.json')
  const manifest = opts.bundledManifest ?? (pkgJsonPath == null ? undefined : await readBundledManifest(pkgJsonPath))
  if (manifest == null) return false
  const depPath = `${manifest.name}@${opts.pkgResolutionId}` as DepPath
  const allowed = opts.allowBuild?.(depPath)
  const ignoredBuild = opts.ignoredBuild ?? opts.filesIndexFile.endsWith('\tnot-built')
  if (allowed === false) return ignoredBuild
  if (allowed === true) return !ignoredBuild
  if (opts.requiresPrepare == null) return false
  resolvePackageBuildPermission({
    allowBuild: opts.allowBuild,
    pkgResolutionId: opts.pkgResolutionId,
  }, manifest)
  return false
}

export async function readBundledManifest (pkgJsonPath: string): Promise<BundledManifest | undefined> {
  return normalizeBundledManifest(await loadJsonFile<DependencyManifest>(pkgJsonPath))
}

export function getExpectedIntegrity (resolution: unknown): string | undefined {
  const integrity = (resolution as { integrity?: unknown }).integrity
  return typeof integrity === 'string' && integrity.length > 0
    ? integrity
    : undefined
}

export function assertFetchableResolution (depPath: string, resolution: AtomicResolution): void {
  if (classifyResolution(resolution) !== 'remoteTarball') return
  if (getExpectedIntegrity(resolution) != null) return
  throw new PnpmError('MISSING_TARBALL_INTEGRITY',
    `Cannot fetch package "${depPath}" from the lockfile: it has no "integrity" field, so the downloaded tarball cannot be verified. Run a fresh install to repair the lockfile.`)
}

export async function tarballIsUpToDate (
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  pkgInStoreLocation: string,
  lockfileDir: string
): Promise<boolean> {
  let currentIntegrity!: string
  try {
    currentIntegrity = (await gfs.readFile(path.join(pkgInStoreLocation, TARBALL_INTEGRITY_FILENAME), 'utf8'))
  } catch (err: any) { // eslint-disable-line
    return false
  }
  if (resolution.integrity && currentIntegrity !== resolution.integrity) return false

  const tarball = path.join(lockfileDir, resolution.tarball.slice(5))
  try {
    await fs.stat(tarball)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      if (!resolution.integrity) return false
      return true
    }
    throw err
  }
  const tarballStream = createReadStream(tarball)
  try {
    return Boolean(await ssri.checkStream(tarballStream, currentIntegrity))
  } catch (err: any) { // eslint-disable-line
    return false
  }
}
