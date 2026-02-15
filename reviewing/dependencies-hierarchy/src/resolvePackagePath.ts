import fs from 'fs'
import path from 'path'
import { depPathToFilename } from '@pnpm/dependency-path'

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
  let fullPackagePath = path.join(
    opts.virtualStoreDir,
    depPathToFilename(opts.depPath, opts.virtualStoreDirMaxLength),
    'node_modules',
    opts.name
  )

  // Resolve symlink for global virtual store.
  // Global virtual store is detected when virtualStoreDir is outside the project's node_modules.
  const resolvedVirtualStoreDir = path.resolve(opts.virtualStoreDir)
  const resolvedModulesDir = opts.modulesDir ? path.resolve(opts.modulesDir) : undefined
  const isGlobalVirtualStore = resolvedModulesDir &&
    !resolvedVirtualStoreDir.startsWith(resolvedModulesDir + path.sep) &&
    resolvedVirtualStoreDir !== resolvedModulesDir

  if (isGlobalVirtualStore) {
    try {
      let nodeModulesDir: string
      if (opts.parentDir) {
        // parentDir example: /store/.../node_modules/express
        //                    /store/.../node_modules/@scope/pkg
        // We need the node_modules directory to find sibling packages
        nodeModulesDir = path.dirname(opts.parentDir)
        // For scoped packages (@org/pkg), go up one more level
        if (path.basename(nodeModulesDir).startsWith('@')) {
          nodeModulesDir = path.dirname(nodeModulesDir)
        }
      } else if (opts.modulesDir) {
        nodeModulesDir = opts.modulesDir
      } else {
        return fullPackagePath
      }
      fullPackagePath = fs.realpathSync(path.join(nodeModulesDir, opts.alias))
    } catch {
      // Fallback to constructed path if symlink doesn't exist
    }
  }

  return fullPackagePath
}
