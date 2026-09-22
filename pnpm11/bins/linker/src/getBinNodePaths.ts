import { promises as fs } from 'node:fs'
import path from 'node:path'

/**
 * Returns the node_modules paths relevant to a binary in the virtual store layout
 * or a custom modules directory.
 * For a binary at `.pnpm/pkg@version/node_modules/pkg/bin/cli.js`, this returns:
 *   1. `.pnpm/pkg@version/node_modules/pkg/node_modules` (bundled dependencies)
 *   2. `.pnpm/pkg@version/node_modules` (sibling/regular dependencies)
 *
 * These directories must be in NODE_PATH so that tools like `import-local`
 * (used by jest, eslint, etc.) which resolve from CWD can find the correct
 * dependency versions.
 */
export async function getBinNodePaths (target: string, modulesDirName: string = 'node_modules'): Promise<string[]> {
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

  let currentDir = dir
  let nodeModulesDir: string | undefined
  while (true) {
    if (path.basename(currentDir) === 'node_modules') {
      if (path.basename(path.dirname(currentDir)) !== 'node_modules') {
        nodeModulesDir = currentDir
        break
      }
    }
    const parent = path.dirname(currentDir)
    if (parent === currentDir) break
    currentDir = parent
  }

  if (nodeModulesDir) {
    return getNodePathsForModulesDir(nodeModulesDir, dir)
  }

  if (modulesDirName !== 'node_modules') {
    const candidates: string[] = []
    currentDir = dir
    while (true) {
      if (path.basename(currentDir) === modulesDirName) {
        if (path.basename(path.dirname(currentDir)) !== modulesDirName) {
          candidates.push(currentDir)
        }
      }
      const parent = path.dirname(currentDir)
      if (parent === currentDir) break
      currentDir = parent
    }

    const modulesDir = candidates.find((candidate) =>
      !candidates.some((other) => other !== candidate && isInsidePackageIn(candidate, other))
    )
    if (modulesDir) {
      return getNodePathsForModulesDir(modulesDir, dir)
    }
  }

  return []
}

function isInsidePackageIn (childDir: string, modulesDir: string): boolean {
  const rel = path.relative(modulesDir, childDir)
  if (!rel || rel.startsWith('..')) return false
  const segments = rel.split(path.sep)
  if (segments[0].startsWith('@')) {
    return segments.length >= 3
  }
  return segments.length >= 2
}

function getNodePathsForModulesDir (modulesDir: string, dir: string): string[] {
  const result: string[] = []
  const rel = path.relative(modulesDir, dir)
  if (rel) {
    const relSegments = rel.split(path.sep)
    const pkgDir = relSegments[0].startsWith('@')
      ? (relSegments.length > 1 ? path.join(modulesDir, relSegments[0], relSegments[1]) : null)
      : path.join(modulesDir, relSegments[0])
    if (pkgDir) {
      result.push(path.join(pkgDir, 'node_modules'))
    }
  }
  result.push(modulesDir)
  return result
}
