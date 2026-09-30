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
  const dir = await realpathIfExists(path.dirname(target))

  const nodeModulesDir = findModulesDirAncestor(dir, 'node_modules', () => true)
  if (nodeModulesDir) {
    return getNodePathsForModulesDir(nodeModulesDir, dir)
  }

  if (path.isAbsolute(modulesDirNameOrPath)) {
    const resolvedModulesDir = await realpathOrSelf(modulesDirNameOrPath)
    const rel = path.relative(resolvedModulesDir, dir)
    if (!rel.startsWith('..') && !path.isAbsolute(rel)) {
      return getNodePathsForModulesDir(resolvedModulesDir, dir)
    }
  }

  const modulesDirName = path.basename(modulesDirNameOrPath)
  if (modulesDirName !== 'node_modules') {
    const customModulesDir = findModulesDirAncestor(dir, modulesDirName, (candidate) => containsPackageDir(candidate, dir))
    if (customModulesDir) {
      return getNodePathsForModulesDir(customModulesDir, dir)
    }
  }

  return []
}

async function realpathIfExists (dir: string): Promise<string> {
  try {
    return await fs.realpath(dir)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return dir
  }
}

async function realpathOrSelf (dir: string): Promise<string> {
  try {
    return await fs.realpath(dir)
  } catch {
    return dir
  }
}

/**
 * Walks up from `dir` and returns the first directory named `modulesDirName`
 * that is not itself nested directly in another `modulesDirName` directory
 * and that satisfies `accept`.
 */
function findModulesDirAncestor (
  dir: string,
  modulesDirName: string,
  accept: (candidate: string) => boolean
): string | undefined {
  let currentDir = dir
  while (true) {
    if (isOutermostModulesDir(currentDir, modulesDirName) && accept(currentDir)) {
      return currentDir
    }
    const parent = path.dirname(currentDir)
    if (parent === currentDir) return undefined
    currentDir = parent
  }
}

function isOutermostModulesDir (candidate: string, modulesDirName: string): boolean {
  return path.basename(candidate) === modulesDirName &&
    path.basename(path.dirname(candidate)) !== modulesDirName
}

function containsPackageDir (modulesDir: string, dir: string): boolean {
  const rel = path.relative(modulesDir, dir)
  if (!rel || rel.startsWith('..')) return false
  const relSegments = rel.split(path.sep)
  const isScoped = relSegments[0].startsWith('@')
  return isScoped ? relSegments.length >= 2 : relSegments.length >= 1
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
