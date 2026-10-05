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
  if (hoistedPaths.length) return pickHoistedDir(hoistedPaths, opts)
  return findHoistedByAlias(opts, lockfileDir)
}

/**
 * Prefers the copy nested under the parent, then the one inside the project,
 * then any copy that exists on disk.
 */
function pickHoistedDir (dirs: string[], opts: { parentDir?: string, projectDir?: string }): string {
  for (const baseDir of [opts.parentDir, opts.projectDir]) {
    if (!baseDir) continue
    const baseDirWithSep = baseDir.endsWith(path.sep) ? baseDir : baseDir + path.sep
    const found = dirs.find((dir) => dir.startsWith(baseDirWithSep) && fs.existsSync(dir))
    if (found) return found
  }
  return dirs.find((dir) => fs.existsSync(dir)) ?? dirs[0]
}

/**
 * Probes `node_modules/<alias>` of the project and of the lockfile directory,
 * because the hoisted linker places a dependency under its alias.
 */
function findHoistedByAlias (opts: ResolvePackagePathOptions, lockfileDir: string): string | undefined {
  if (isUnsafePathComponent(opts.alias)) return undefined
  const relativeModulesDir = getRelativeModulesDir(lockfileDir, opts.modulesDir)
  return [opts.projectDir, lockfileDir]
    .filter((dir): dir is string => Boolean(dir))
    .map((dir) => path.resolve(dir, relativeModulesDir, opts.alias))
    .find((candidate) => candidateMatchesVersion(candidate, opts.version))
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
