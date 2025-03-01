import { existsSync } from 'fs'
import path from 'path'
import { createLocalTarballFileIntegrity } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  type DirectoryResolution,
  type ResolveResult,
  type TarballResolution,
} from '@pnpm/resolver-base'
import { type DependencyManifest } from '@pnpm/types'
import { logger } from '@pnpm/logger'
import { parsePref, type WantedLocalDependency } from './parsePref'

export type { WantedLocalDependency }

export interface ResolveFromLocalResult extends ResolveResult {
  normalizedPref: string
  resolution: TarballResolution | DirectoryResolution
  manifest?: DependencyManifest
}

/**
 * Resolves a package hosted on the local filesystem
 */
export async function resolveFromLocal (
  wantedDependency: WantedLocalDependency,
  opts: {
    lockfileDir?: string
    projectDir: string
  }
): Promise<ResolveFromLocalResult | null> {
  const spec = parsePref(wantedDependency, opts.projectDir, opts.lockfileDir ?? opts.projectDir)
  if (spec == null) return null
  if (spec.type === 'file') {
    return {
      id: spec.id,
      normalizedPref: spec.normalizedPref,
      resolution: {
        integrity: await createLocalTarballFileIntegrity(spec.fetchSpec),
        tarball: spec.id,
      },
      resolvedVia: 'local-filesystem',
    }
  }

  let localDependencyManifest!: DependencyManifest
  try {
    localDependencyManifest = await readProjectManifestOnly(spec.fetchSpec) as DependencyManifest
  } catch (internalErr: any) { // eslint-disable-line
    if (!existsSync(spec.fetchSpec)) {
      if (spec.id.startsWith('file:')) {
        throw new PnpmError('LINKED_PKG_DIR_NOT_FOUND',
          `Could not install from "${spec.fetchSpec}" as it does not exist.`)
      }
      logger.warn({
        message: `Installing a dependency from a non-existent directory: ${spec.fetchSpec}`,
        prefix: opts.projectDir,
      })
      localDependencyManifest = {
        name: path.basename(spec.fetchSpec),
        version: '0.0.0',
      }
    } else {
      switch (internalErr.code) {
      case 'ENOTDIR': {
        throw new PnpmError('NOT_PACKAGE_DIRECTORY',
          `Could not install from "${spec.fetchSpec}" as it is not a directory.`)
      }
      case 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND':
      case 'ENOENT': {
        localDependencyManifest = {
          name: path.basename(spec.fetchSpec),
          version: '0.0.0',
        }
        break
      }
      default: {
        throw internalErr
      }
      }
    }
  }
  return {
    id: spec.id,
    manifest: localDependencyManifest,
    normalizedPref: spec.normalizedPref,
    resolution: {
      directory: spec.dependencyPath,
      type: 'directory',
    },
    resolvedVia: 'local-filesystem',
  }
}
