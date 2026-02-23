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
  const segments = dir.split(path.sep)
  // Walk from the innermost directory outward to find the first non-nested node_modules
  for (let i = segments.length - 1; i >= 0; i--) {
    if (segments[i] !== 'node_modules') continue
    // Skip nested node_modules (e.g., node_modules/node_modules)
    if (i > 0 && segments[i - 1] === 'node_modules') continue

    const nodeModulesDir = segments.slice(0, i + 1).join(path.sep)
    const result: string[] = []
    if (i + 1 < segments.length) {
      // For scoped packages, the package dir is two levels deep: @scope/pkg
      const pkgDepth = segments[i + 1].startsWith('@') ? 3 : 2
      const pkgDir = segments.slice(0, i + pkgDepth).join(path.sep)
      result.push(path.join(pkgDir, 'node_modules'))
    }
    result.push(nodeModulesDir)
    return result
  }
  return []
}
