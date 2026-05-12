import { existsSync } from 'node:fs'
import path from 'node:path'

import { getTarballIntegrity } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { DirectoryResolution, Resolution, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, PkgResolutionId } from '@pnpm/types'
import { readProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'

import { type LocalPackageSpec, parseLocalPath, parseLocalScheme, type WantedLocalDependency } from './parseBareSpecifier.js'

export { type WantedLocalDependency }

export interface LocalResolveResult extends ResolveResult {
  manifest?: DependencyManifest
  normalizedBareSpecifier?: string
  resolution: DirectoryResolution | TarballResolution
  resolvedVia: 'local-filesystem'
}

export interface LocalResolverContext {
  preserveAbsolutePaths?: boolean
}

export interface LocalResolverOptions {
  lockfileDir?: string
  projectDir: string
  currentPkg?: {
    id: PkgResolutionId
    resolution: DirectoryResolution | TarballResolution | Resolution
  }
  update?: false | 'compatible' | 'latest'
}

/**
 * Resolves a dependency declared with an explicit local scheme:
 * `link:`, `workspace:`, `file:`, or (rejected) `path:`.
 */
export async function resolveFromLocalScheme (
  ctx: LocalResolverContext,
  wantedDependency: WantedLocalDependency,
  opts: LocalResolverOptions
): Promise<LocalResolveResult | null> {
  const spec = parseLocalScheme(wantedDependency, opts.projectDir, opts.lockfileDir ?? opts.projectDir, {
    preserveAbsolutePaths: ctx.preserveAbsolutePaths ?? false,
  })
  return resolveSpec(spec, opts)
}

/**
 * Resolves a dependency by path shape — a relative/absolute path or a tarball
 * filename. Does not look at scheme prefixes; callers that want scheme support
 * should call {@link resolveFromLocalScheme} first.
 */
export async function resolveFromLocalPath (
  ctx: LocalResolverContext,
  wantedDependency: WantedLocalDependency,
  opts: LocalResolverOptions
): Promise<LocalResolveResult | null> {
  const spec = parseLocalPath(wantedDependency, opts.projectDir, opts.lockfileDir ?? opts.projectDir, {
    preserveAbsolutePaths: ctx.preserveAbsolutePaths ?? false,
  })
  return resolveSpec(spec, opts)
}

async function resolveSpec (
  spec: LocalPackageSpec | null,
  opts: LocalResolverOptions
): Promise<LocalResolveResult | null> {
  if (spec == null) return null

  if (spec.type === 'file') {
    const integrity = await getTarballIntegrity(spec.fetchSpec)
    return {
      id: spec.id,
      normalizedBareSpecifier: spec.normalizedBareSpecifier,
      resolution: {
        integrity,
        tarball: spec.id,
      },
      resolvedVia: 'local-filesystem',
    }
  }

  // Skip resolution if we have a current package and not updating
  if (opts.currentPkg?.resolution && spec.type === 'directory' && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as DirectoryResolution,
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
