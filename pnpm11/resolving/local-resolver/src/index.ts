import { existsSync } from 'node:fs'
import path from 'node:path'

import { getTarballIntegrity } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { logger } from '@pnpm/logger'
import type { DirectoryResolution, LatestInfo, LatestQuery, Resolution, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, PkgResolutionId } from '@pnpm/types'
import { readProjectManifestOnly, safeReadParentPublishManifest } from '@pnpm/workspace.project-manifest-reader'

import { barePathIsUnambiguous, isDriveLetterPrefix, isFilespec, isLocalFilesystemSpecifier, isTarballFilename, linkedDirectoryPath, localFilePath, type LocalPackageSpec, parseLocalPath, parseLocalScheme, type WantedLocalDependency } from './parseBareSpecifier.js'

export { barePathIsUnambiguous, isDriveLetterPrefix, isFilespec, isLocalFilesystemSpecifier, isTarballFilename, linkedDirectoryPath, localFilePath, type WantedLocalDependency }
export { fileSpecToPackageRootLink } from './packageRootLink.js'

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
  injectWorkspacePackages?: boolean
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
    injectWorkspacePackages: opts.injectWorkspacePackages ?? false,
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
  return resolveSpec(spec, opts, wantedDependency.bareSpecifier)
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

/**
 * @param pathShapedSpecifier - The written specifier, when the spec was
 * claimed by its path shape alone.
 */
async function resolveSpec (
  spec: LocalPackageSpec | null,
  opts: LocalResolverOptions,
  pathShapedSpecifier?: string
): Promise<LocalResolveResult | null> {
  if (spec == null) return null

  if (spec.type === 'file') {
    return resolveTarballSpec(spec, opts)
  }

  // Skip resolution if we have a current package and not updating
  if (opts.currentPkg?.resolution && spec.type === 'directory' && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as DirectoryResolution,
      resolvedVia: 'local-filesystem',
    }
  }

  const localDependencyManifest = await readLocalDependencyManifest(spec, opts, pathShapedSpecifier)
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

async function resolveTarballSpec (
  spec: LocalPackageSpec,
  opts: LocalResolverOptions
): Promise<LocalResolveResult> {
  let integrity: string
  try {
    integrity = await getTarballIntegrity(spec.fetchSpec)
  } catch (err: unknown) {
    const mayReuseLockedTarball = !opts.update && (err as { code?: string })?.code === 'ENOENT'
    if (
      mayReuseLockedTarball &&
      opts.currentPkg?.resolution &&
      'tarball' in opts.currentPkg.resolution &&
      Boolean(opts.currentPkg.resolution.integrity) &&
      (opts.currentPkg.id === spec.id || opts.currentPkg.resolution.tarball === spec.id)
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

async function readLocalDependencyManifest (
  spec: LocalPackageSpec,
  opts: LocalResolverOptions,
  pathShapedSpecifier: string | undefined
): Promise<DependencyManifest> {
  try {
    return (await readProjectManifestOnly(spec.fetchSpec)) as DependencyManifest
  } catch (internalErr: unknown) {
    if (!existsSync(spec.fetchSpec)) {
      throwIfUnsupportedProtocol(pathShapedSpecifier)
      return handleMissingLocalDir(spec, opts.projectDir)
    }
    return handleExistingDirManifestError(internalErr, spec.fetchSpec)
  }
}

/**
 * A specifier claimed by its path shape that opens with a protocol of two or
 * more characters, such as Yarn's `patch:`, names a protocol no resolver
 * supports. Called only once the directory is found missing.
 */
function throwIfUnsupportedProtocol (specifier: string | undefined): void {
  if (specifier == null) return
  const protocolEnd = specifier.indexOf(':')
  if (protocolEnd < 2) return
  const scheme = specifier.slice(0, protocolEnd)
  if (!isAsciiLetter(scheme[0]) || ![...scheme].every(isSchemeChar)) return
  throw new PnpmError(
    'UNSUPPORTED_PROTOCOL',
    `Unsupported protocol "${scheme}:" in the dependency specifier "${specifier}"`,
    { hint: 'If a published package declares this dependency, replace its specifier with the "overrides" setting.' }
  )
}

function isAsciiLetter (char: string): boolean {
  return (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z')
}

function isSchemeChar (char: string): boolean {
  return isAsciiLetter(char) || (char >= '0' && char <= '9') || char === '+' || char === '-' || char === '.'
}

function handleMissingLocalDir (spec: LocalPackageSpec, projectDir: string): DependencyManifest {
  if (spec.id.startsWith('file:')) {
    throw new PnpmError('LINKED_PKG_DIR_NOT_FOUND', `Could not install from "${spec.fetchSpec}" as it does not exist.`)
  }
  logger.warn({
    message: `Installing a dependency from a non-existent directory: ${spec.fetchSpec}`,
    prefix: projectDir,
  })
  return {
    name: path.basename(spec.fetchSpec),
  } as DependencyManifest
}

async function handleExistingDirManifestError (internalErr: unknown, fetchSpec: string): Promise<DependencyManifest> {
  const errCode = (internalErr as { code?: string })?.code
  if (errCode === 'ENOTDIR') {
    throw new PnpmError('NOT_PACKAGE_DIRECTORY', `Could not install from "${fetchSpec}" as it is not a directory.`)
  }
  if (errCode === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND' || errCode === 'ENOENT') {
    const parentManifest = (await safeReadParentPublishManifest(fetchSpec)) as DependencyManifest | null
    if (parentManifest) {
      return parentManifest
    }
    return {
      name: path.basename(fetchSpec),
    } as DependencyManifest
  }
  throw internalErr
}

