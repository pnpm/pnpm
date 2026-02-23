import { promises as fs } from 'fs'
import path from 'path'

/**
 * Returns the node_modules paths relevant to a binary in the virtual store layout.
 * For a binary at `.pnpm/pkg@version/node_modules/pkg/bin/cli.js`, this returns:
 *   1. `.pnpm/pkg@version/node_modules/pkg/node_modules` (bundled dependencies)
 *   2. `.pnpm/pkg@version/node_modules` (sibling/regular dependencies)
 *
 * These directories must be in NODE_PATH so that tools like `import-local`
 * (used by jest, eslint, etc.) which resolve from CWD can find the correct
 * dependency versions.
 */
export async function getBinNodePaths (target: string): Promise<string[]> {
  const targetDir = path.dirname(target)
  let dir: string
  try {
    dir = await fs.realpath(targetDir)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    dir = targetDir
  }
  // Walk up from the resolved directory to find the first non-nested node_modules
  let currentDir = dir
  while (true) {
    if (path.basename(currentDir) === 'node_modules') {
      // Skip nested node_modules (e.g., node_modules/node_modules)
      if (path.basename(path.dirname(currentDir)) !== 'node_modules') {
        const nodeModulesDir = currentDir
        const result: string[] = []

        // Determine the package directory from the relative path between
        // node_modules and the resolved binary directory
        const rel = path.relative(nodeModulesDir, dir)
        if (rel) {
          const relSegments = rel.split(path.sep)
          // For scoped packages, the package dir is two levels deep: @scope/pkg
          const pkgDir = relSegments[0].startsWith('@')
            ? path.join(nodeModulesDir, relSegments[0], relSegments[1])
            : path.join(nodeModulesDir, relSegments[0])
          result.push(path.join(pkgDir, 'node_modules'))
        }

        result.push(nodeModulesDir)
        return result
      }
    }
    const parent = path.dirname(currentDir)
    if (parent === currentDir) break
    currentDir = parent
  }
  return []
}
