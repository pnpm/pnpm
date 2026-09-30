import fs from 'node:fs'
import path from 'node:path'

import { depPathToFilename } from '@pnpm/deps.path'

/**
 * Resolves the filesystem path for a package identified by its depPath.
 *
 * For local virtual stores, the path is constructed directly.
 * For global virtual stores (where virtualStoreDir is outside modulesDir),
 * symlinks are resolved to find the actual store location.
 */
export function resolvePackagePath (opts: {
  depPath: string
  name: string
  alias: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  parentDir?: string
}): string {
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
