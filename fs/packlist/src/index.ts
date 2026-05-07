import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import npmPacklist from 'npm-packlist'

interface Edge {
  to: TreeNode
  peer: boolean
  dev: boolean
}

interface TreeNode {
  path: string
  package: Record<string, unknown>
  isProjectRoot?: boolean
  isLink: boolean
  target: TreeNode
  edgesOut: Map<string, Edge>
}

export async function packlist (pkgDir: string, opts?: {
  manifest?: Record<string, unknown>
}): Promise<string[]> {
  const pkg = opts?.manifest ?? await readPackageJson(pkgDir)
  const tree = await buildTree(pkgDir, pkg, true, new Map())
  const files = await npmPacklist(tree)
  return files.map((file) => file.replace(/^\.[/\\]/, ''))
}

async function buildTree (
  pkgDir: string,
  pkg: Record<string, unknown>,
  isProjectRoot: boolean,
  seen: Map<string, TreeNode>
): Promise<TreeNode> {
  const cached = seen.get(pkgDir)
  if (cached) return cached
  const depsToBundle = getDepsToBundle(pkg, isProjectRoot)
  // npm-packlist's gatherBundles() reads bundleDependencies from the package directly
  // and does `for (const dep of bundleDependencies)`, so it must be an iterable.
  // Normalize true/undefined to an explicit array.
  const normalizedPkg = normalizePackage(pkg)
  if (isProjectRoot) {
    normalizedPkg.bundleDependencies = depsToBundle
    delete normalizedPkg.bundledDependencies
  }
  const node = {
    path: pkgDir,
    package: normalizedPkg,
    isProjectRoot,
    isLink: false,
    edgesOut: new Map<string, Edge>(),
  } as TreeNode
  node.target = node
  seen.set(pkgDir, node)
  // Sequential to keep the shared `seen` map deduplication race-free.
  for (const dep of depsToBundle) {
    // eslint-disable-next-line no-await-in-loop
    const depDir = await resolveDependency(dep, pkgDir)
    if (!depDir) continue
    // eslint-disable-next-line no-await-in-loop
    const depPkg = await readPackageJson(depDir)
    // eslint-disable-next-line no-await-in-loop
    const depNode = await buildTree(depDir, depPkg, false, seen)
    node.edgesOut.set(dep, { to: depNode, peer: false, dev: false })
  }
  return node
}

function getDepsToBundle (pkg: Record<string, unknown>, isProjectRoot: boolean): string[] {
  if (isProjectRoot) {
    const bundle = pkg.bundleDependencies ?? pkg.bundledDependencies
    if (Array.isArray(bundle)) return bundle as string[]
    if (bundle === true) {
      const dependencies = (pkg.dependencies ?? {}) as Record<string, string>
      return Object.keys(dependencies)
    }
    return []
  }
  const dependencies = (pkg.dependencies ?? {}) as Record<string, string>
  const optionalDependencies = (pkg.optionalDependencies ?? {}) as Record<string, string>
  return [...Object.keys(dependencies), ...Object.keys(optionalDependencies)]
}

async function resolveDependency (depName: string, fromDir: string): Promise<string | undefined> {
  let currentDir = fromDir
  while (true) {
    const candidate = path.join(currentDir, 'node_modules', depName)
    try {
      // eslint-disable-next-line no-await-in-loop
      const stat = await fs.stat(path.join(candidate, 'package.json'))
      if (stat.isFile()) return candidate
    } catch (err: unknown) {
      if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
        throw err
      }
    }
    const parent = path.dirname(currentDir)
    if (parent === currentDir) return undefined
    currentDir = parent
  }
}

async function readPackageJson (dir: string): Promise<Record<string, unknown>> {
  try {
    return JSON.parse(await fs.readFile(path.join(dir, 'package.json'), 'utf8'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return {}
    }
    throw err
  }
}

function stripDotSlash (p: string): string {
  return p.replace(/^\.[/\\]/, '')
}

function normalizePackage (pkg: Record<string, unknown>): Record<string, unknown> {
  const normalized = { ...pkg }
  if (typeof normalized.main === 'string') {
    normalized.main = stripDotSlash(normalized.main)
  }
  if (typeof normalized.browser === 'string') {
    normalized.browser = stripDotSlash(normalized.browser)
  }
  if (typeof normalized.bin === 'string') {
    normalized.bin = stripDotSlash(normalized.bin)
  } else if (normalized.bin != null && typeof normalized.bin === 'object') {
    const bin: Record<string, string> = {}
    for (const [key, value] of Object.entries(normalized.bin as Record<string, string>)) {
      bin[key] = stripDotSlash(value)
    }
    normalized.bin = bin
  }
  return normalized
}
