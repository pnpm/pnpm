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
export async function getBinNodePaths (target: string, modulesDirNameOrPath: string = 'node_modules'): Promise<string[]> {
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

  if (path.isAbsolute(modulesDirNameOrPath)) {
    let resolvedModulesDir: string
    try {
      resolvedModulesDir = await fs.realpath(modulesDirNameOrPath)
    } catch {
      resolvedModulesDir = modulesDirNameOrPath
    }
    const rel = path.relative(resolvedModulesDir, dir)
    if (!rel.startsWith('..') && !path.isAbsolute(rel)) {
      return getNodePathsForModulesDir(resolvedModulesDir, dir)
    }
  }

  const modulesDirName = path.basename(modulesDirNameOrPath)
  if (modulesDirName !== 'node_modules') {
    currentDir = dir
    while (true) {
      if (path.basename(currentDir) === modulesDirName) {
        if (path.basename(path.dirname(currentDir)) !== modulesDirName) {
          const rel = path.relative(currentDir, dir)
          if (rel && !rel.startsWith('..')) {
            const relSegments = rel.split(path.sep)
            const isScoped = relSegments[0].startsWith('@')
            if (isScoped ? relSegments.length >= 2 : relSegments.length >= 1) {
              return getNodePathsForModulesDir(currentDir, dir)
            }
          }
        }
      }
      const parent = path.dirname(currentDir)
      if (parent === currentDir) break
      currentDir = parent
    }
  }

  return []
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
