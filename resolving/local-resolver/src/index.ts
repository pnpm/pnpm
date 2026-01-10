import { existsSync } from 'fs'
import path from 'path'
import { getTarballIntegrity } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import { type DirectoryResolution, type Resolution, type ResolveResult, type TarballResolution } from '@pnpm/resolver-base'
import { type DependencyManifest } from '@pnpm/types'
import { logger } from '@pnpm/logger'
import { parseBareSpecifier, type WantedLocalDependency } from './parseBareSpecifier.js'

export { type WantedLocalDependency }

export interface LocalResolveResult extends ResolveResult {
  manifest?: DependencyManifest
  normalizedBareSpecifier: string
  resolution: DirectoryResolution | TarballResolution
  resolvedVia: 'local-filesystem'
}

/**
 * Resolves a package hosted on the local filesystem
 */
export async function resolveFromLocal (
  ctx: {
    preserveAbsolutePaths?: boolean
  },
  wantedDependency: WantedLocalDependency,
  opts: {
    lockfileDir?: string
    projectDir: string
    currentPkg?: {
      id: string
      resolution: DirectoryResolution | TarballResolution | Resolution
    }
    update?: false | 'compatible' | 'latest'
  }
): Promise<LocalResolveResult | null> {
  const preserveAbsolutePaths = ctx.preserveAbsolutePaths ?? false
  const spec = parseBareSpecifier(wantedDependency, opts.projectDir, opts.lockfileDir ?? opts.projectDir, { preserveAbsolutePaths })
  if (spec == null) return null

  // Skip resolution if we have a current package and not updating
  if (opts.currentPkg && !opts.update && opts.currentPkg.id === spec.id) {
    const currentResolution = opts.currentPkg.resolution

    // For file: tarballs, check if integrity changed
    if (spec.type === 'file' && currentResolution.type == null) {
      const currentIntegrity = await getTarballIntegrity(spec.fetchSpec)
      const previousIntegrity = (currentResolution as TarballResolution).integrity

      if (currentIntegrity === previousIntegrity) {
        // Skip resolution - return existing resolution
        return {
          id: spec.id,
          normalizedBareSpecifier: spec.normalizedBareSpecifier,
          resolution: currentResolution as TarballResolution,
          resolvedVia: 'local-filesystem',
        }
      }
    }

    // For directories, if the ID matches, we can skip
    if (currentResolution.type === 'directory') {
      return {
        id: spec.id,
        normalizedBareSpecifier: spec.normalizedBareSpecifier,
        resolution: currentResolution,
        resolvedVia: 'local-filesystem',
      }
    }
  }

  if (spec.type === 'file') {
    return {
      id: spec.id,
      normalizedBareSpecifier: spec.normalizedBareSpecifier,
      resolution: {
        integrity: await getTarballIntegrity(spec.fetchSpec),
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
    normalizedBareSpecifier: spec.normalizedBareSpecifier,
    resolution: {
      directory: spec.dependencyPath,
      type: 'directory',
    },
    resolvedVia: 'local-filesystem',
  }
}
