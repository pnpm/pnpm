import { existsSync } from 'node:fs'
import path from 'node:path'

import { getTarballIntegrity } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { DirectoryResolution, LatestInfo, LatestQuery, Resolution, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, PkgResolutionId } from '@pnpm/types'
import { readProjectManifestOnly, safeReadPublishManifest } from '@pnpm/workspace.project-manifest-reader'

import { barePathIsUnambiguous, isDriveLetterPrefix, isFilespec, isLocalFilesystemSpecifier, isTarballFilename, type LocalPackageSpec, parseLocalPath, parseLocalScheme, type WantedLocalDependency } from './parseBareSpecifier.js'

export { barePathIsUnambiguous, isDriveLetterPrefix, isFilespec, isLocalFilesystemSpecifier, isTarballFilename, type WantedLocalDependency }

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

// link:/file:/workspace: dependencies don't have a "latest" — claim them so
// the dispatcher stops here. Returning undefined would let downstream
// resolvers try; in particular, a user-configured named-registry alias
// called `link`, `file`, or `workspace` could otherwise hijack these.
export async function resolveLatestFromLocal (query: LatestQuery): Promise<LatestInfo | undefined> {
  const spec = query.wantedDependency.bareSpecifier
  if (spec?.startsWith('link:') || spec?.startsWith('file:') || spec?.startsWith('workspace:')) {
    return {}
  }
  return undefined
}

async function resolveSpec (
  spec: LocalPackageSpec | null,
  opts: LocalResolverOptions
): Promise<LocalResolveResult | null> {
  if (spec == null) return null

  if (spec.type === 'file') {
    let integrity: string
    try {
      integrity = await getTarballIntegrity(spec.fetchSpec)
    } catch (err: unknown) {
      if (
        opts.currentPkg?.resolution &&
        'tarball' in opts.currentPkg.resolution &&
        Boolean(opts.currentPkg.resolution.integrity) &&
        (opts.currentPkg.id === spec.id || opts.currentPkg.resolution.tarball === spec.id) &&
        !opts.update &&
        (err as { code?: string })?.code === 'ENOENT'
      ) {
        return {
          id: spec.id,
          normalizedBareSpecifier: spec.normalizedBareSpecifier,
          resolution: opts.currentPkg.resolution as TarballResolution,
          resolvedVia: 'local-filesystem',
        }
      }
      throw err
    }
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
          const parentManifest = await safeReadPublishManifest(spec.fetchSpec) as DependencyManifest | null
          if (parentManifest) {
            localDependencyManifest = parentManifest
            break
          }
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
