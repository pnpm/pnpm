import fs from 'node:fs'
import path from 'node:path'

import { depPathToFilename, removeSuffix } from '@pnpm/deps.path'
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
export function resolvePackagePath (opts: {
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
}): string {
  if (isUnsafePathComponent(opts.name)) {
    return opts.virtualStoreDir
  }
  if (opts.nodeLinker === 'hoisted') {
    const lockfileDir = opts.lockfileDir ?? opts.projectDir
    if (lockfileDir) {
      const locations = opts.hoistedLocations?.[opts.depPath] ??
        opts.hoistedLocations?.[removeSuffix(opts.depPath)] ??
        (opts.depPath.startsWith('/') ? opts.hoistedLocations?.[opts.depPath.slice(1)] : opts.hoistedLocations?.[`/${opts.depPath}`]) ??
        []
      const hoistedPaths = locations
        .map((location) => hoistedPackageDir(lockfileDir, location))
        .filter((location): location is string => location != null)
      if (hoistedPaths.length) {
        if (opts.parentDir) {
          const parentDirWithSep = opts.parentDir.endsWith(path.sep) ? opts.parentDir : opts.parentDir + path.sep
          const nested = hoistedPaths.find((loc) => loc.startsWith(parentDirWithSep) && fs.existsSync(loc))
          if (nested) return nested
        }
        if (opts.projectDir) {
          const projectDirWithSep = opts.projectDir.endsWith(path.sep) ? opts.projectDir : opts.projectDir + path.sep
          const inProject = hoistedPaths.find((loc) => (loc === opts.projectDir || loc.startsWith(projectDirWithSep)) && fs.existsSync(loc))
          if (inProject) return inProject
        }
        const existing = hoistedPaths.find((loc) => fs.existsSync(loc))
        if (existing) return existing
        return hoistedPaths[0]
      }
      const modulesDirName = opts.modulesDir ? path.basename(opts.modulesDir) : 'node_modules'
      if (opts.projectDir) {
        const candidateInProject = path.resolve(opts.projectDir, modulesDirName, opts.name)
        if (candidateMatchesVersion(candidateInProject, opts.version)) {
          return candidateInProject
        }
      }
      const candidateInLockfile = path.resolve(lockfileDir, modulesDirName, opts.name)
      if (candidateMatchesVersion(candidateInLockfile, opts.version)) {
        return candidateInLockfile
      }
    }
  }

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

function hoistedPackageDir (lockfileDir: string, location: string | undefined): string | undefined {
  if (location == null || path.isAbsolute(location) || location.startsWith('\\')) return undefined
  const dir = path.join(lockfileDir, ...location.split(/[/\\]/))
  const relative = path.relative(lockfileDir, dir)
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) return undefined
  return dir
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

