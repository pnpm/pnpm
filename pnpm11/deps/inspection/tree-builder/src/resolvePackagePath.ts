import fs from 'node:fs'
import path from 'node:path'

import { depPathToFilename, findHoistedPackageDirs } from '@pnpm/deps.path'
import { readPackageJsonFromDirSync } from '@pnpm/pkg-manifest.reader'

/**
 * Resolves the filesystem path for a package identified by its depPath.
 *
 * For local virtual stores, the path is constructed directly.
 * For global virtual stores (where virtualStoreDir is outside modulesDir),
 * symlinks are resolved to find the actual store location.
 * For hoisted linker (where virtual store is empty), packages live where the
 * hoisted linker placed them, recorded in hoistedLocations.
 */
export interface ResolvePackagePathOptions {
  depPath: string
  name: string
  alias: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  parentDir?: string
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  hoistedLocations?: Record<string, string[]>
  lockfileDir?: string
  projectDir?: string
  version?: string
}

export function resolvePackagePath (opts: ResolvePackagePathOptions): string {
  if (isUnsafePathComponent(opts.name)) {
    return opts.virtualStoreDir
  }
  if (opts.nodeLinker === 'hoisted') {
    const hoistedPath = resolveHoistedPackagePath(opts)
    if (hoistedPath != null) return hoistedPath
  }
  return resolveVirtualStorePackagePath(opts)
}

function resolveHoistedPackagePath (opts: ResolvePackagePathOptions): string | undefined {
  const lockfileDir = opts.lockfileDir ?? opts.projectDir
  if (!lockfileDir) return undefined
  const hoistedPaths = findHoistedPackageDirs(opts.hoistedLocations, opts.depPath, lockfileDir)
  if (hoistedPaths.length) return pickHoistedDir(hoistedPaths, { ...opts, lockfileDir })
  return findHoistedByAlias(opts, lockfileDir)
}

/**
 * Picks the copy that Node.js resolves from the parent package, or from the
 * project for a direct dependency: the one in the closest modules directory
 * above it. Copies on disk under the edge's alias are preferred, since one
 * depPath gets a directory per alias it is installed under. Falls back to any
 * copy on disk, then to the recorded one under the alias, then to the first
 * recorded one.
 */
function pickHoistedDir (recordedDirs: string[], opts: ResolvePackagePathOptions & { lockfileDir: string }): string {
  const existing = recordedDirs.filter((dir) => fs.existsSync(dir))
  const existingUnderAlias = existing.filter((dir) => isInstalledUnder(dir, opts.alias))
  const candidates = existingUnderAlias.length ? existingUnderAlias : existing
  const resolveFrom = opts.parentDir ?? opts.projectDir
  if (resolveFrom) {
    const relativeModulesDir = getRelativeModulesDir(opts.lockfileDir, opts.modulesDir)
    const closest = candidates
      .map((dir) => ({ dir, ownerDir: getOwnerDir(dir, relativeModulesDir) }))
      .filter(({ ownerDir }) => isSameOrSubdir(ownerDir, resolveFrom))
      .sort((a, b) => b.ownerDir.length - a.ownerDir.length)[0]
    if (closest) return closest.dir
  }
  return candidates[0] ??
    recordedDirs.find((dir) => isInstalledUnder(dir, opts.alias)) ??
    recordedDirs[0]
}

/**
 * Whether `pkgDir` is `<modules dir>/<alias>`: its trailing segments spell the
 * whole alias, not just the name of a scoped package.
 */
function isInstalledUnder (pkgDir: string, alias: string): boolean {
  const aliasSuffix = path.join(path.sep, alias)
  return pkgDir.endsWith(aliasSuffix) &&
    !path.basename(pkgDir.slice(0, -aliasSuffix.length)).startsWith('@')
}

/**
 * The directory whose modules directory holds `pkgDir`.
 */
function getOwnerDir (pkgDir: string, relativeModulesDir: string): string {
  let modulesDir = path.dirname(pkgDir)
  if (path.basename(modulesDir).startsWith('@')) modulesDir = path.dirname(modulesDir)
  const rootModulesDirSuffix = path.sep + relativeModulesDir
  return modulesDir.endsWith(rootModulesDirSuffix)
    ? modulesDir.slice(0, -rootModulesDirSuffix.length)
    : path.dirname(modulesDir)
}

function isSameOrSubdir (parent: string, dir: string): boolean {
  return dir === parent || dir.startsWith(parent.endsWith(path.sep) ? parent : parent + path.sep)
}

/**
 * Probes `<alias>` in the modules directory of the project and of the root,
 * because the hoisted linker places a dependency under its alias.
 */
function findHoistedByAlias (opts: ResolvePackagePathOptions, lockfileDir: string): string | undefined {
  if (isUnsafePathComponent(opts.alias)) return undefined
  const relativeModulesDir = getRelativeModulesDir(lockfileDir, opts.modulesDir)
  const rootModulesDir = opts.modulesDir ?? path.join(lockfileDir, 'node_modules')
  const candidates = opts.projectDir ? [path.resolve(opts.projectDir, relativeModulesDir, opts.alias)] : []
  candidates.push(path.join(rootModulesDir, opts.alias))
  return candidates.find((candidate) => candidateMatchesVersion(candidate, opts.version))
}

/**
 * The modules directory relative to the lockfile directory, such as
 * `node_modules` or `www/modules`. Falls back to its last segment when it
 * lies outside the lockfile directory.
 */
function getRelativeModulesDir (lockfileDir: string, modulesDir: string | undefined): string {
  if (!modulesDir) return 'node_modules'
  const relative = path.relative(lockfileDir, modulesDir)
  if (!relative || relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    return path.basename(modulesDir)
  }
  return relative
}

function resolveVirtualStorePackagePath (opts: ResolvePackagePathOptions): string {
  const fullPackagePath = path.join(
    opts.virtualStoreDir,
    depPathToFilename(opts.depPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    opts.name
  )

  // Resolve symlink for global virtual store.
  if (!isGlobalVirtualStore(opts.virtualStoreDir, opts.modulesDir)) return fullPackagePath
  const nodeModulesDir = getSymlinkNodeModulesDir(opts)
  if (nodeModulesDir == null) return fullPackagePath
  try {
    return fs.realpathSync(path.join(nodeModulesDir, opts.alias))
  } catch {
    // Fallback to constructed path if symlink doesn't exist
    return fullPackagePath
  }
}

/**
 * Global virtual store is detected when virtualStoreDir is outside the project's node_modules.
 */
function isGlobalVirtualStore (virtualStoreDir: string, modulesDir: string | undefined): boolean {
  const resolvedVirtualStoreDir = path.resolve(virtualStoreDir)
  const resolvedModulesDir = modulesDir ? path.resolve(modulesDir) : undefined
  return Boolean(resolvedModulesDir &&
    !resolvedVirtualStoreDir.startsWith(resolvedModulesDir + path.sep) &&
    resolvedVirtualStoreDir !== resolvedModulesDir)
}

function getSymlinkNodeModulesDir (opts: { modulesDir?: string, parentDir?: string }): string | undefined {
  if (!opts.parentDir) return opts.modulesDir || undefined
  // parentDir example: /store/.../node_modules/express
  //                    /store/.../node_modules/@scope/pkg
  // We need the node_modules directory to find sibling packages
  const nodeModulesDir = path.dirname(opts.parentDir)
  // For scoped packages (@org/pkg), go up one more level
  return path.basename(nodeModulesDir).startsWith('@')
    ? path.dirname(nodeModulesDir)
    : nodeModulesDir
}

function candidateMatchesVersion (candidate: string, expectedVersion: string | undefined): boolean {
  if (!expectedVersion) return fs.existsSync(candidate)
  try {
    const manifest = readPackageJsonFromDirSync(candidate)
    return manifest.version === expectedVersion
  } catch {
    return false
  }
}

function isUnsafePathComponent (component: string): boolean {
  return (
    path.isAbsolute(component) ||
    component.startsWith('\\') ||
    component.includes(':') ||
    component.split(/[/\\]/).some((part) => part === '..' || part === '.')
  )
}
