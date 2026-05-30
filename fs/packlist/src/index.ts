import fs from 'node:fs'
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
  const pkg = opts?.manifest ?? readPackageJson(pkgDir)
  const tree = buildRootTree(pkgDir, pkg)
  const files = await npmPacklist(tree)
  return files.map((file) => file.replace(/^\.[/\\]/, ''))
}

function buildRootTree (pkgDir: string, pkg: Record<string, unknown>): TreeNode {
  const bundledDeps = getRootBundledDeps(pkg)
  // npm-packlist's gatherBundles() iterates package.bundleDependencies directly,
  // so the field must be an array. Normalize true/undefined to an explicit list.
  const normalizedPkg = normalizePackage(pkg)
  normalizedPkg.bundleDependencies = bundledDeps
  delete normalizedPkg.bundledDependencies
  const root = makeNode(pkgDir, normalizedPkg, true)
  const seen = new Map<string, TreeNode>([[pkgDir, root]])
  populateEdges(root, bundledDeps, seen)
  return root
}

function buildBundledTree (pkgDir: string, seen: Map<string, TreeNode>): TreeNode {
  const cached = seen.get(pkgDir)
  if (cached) return cached
  const pkg = readPackageJson(pkgDir)
  const node = makeNode(pkgDir, normalizePackage(pkg), false)
  seen.set(pkgDir, node)
  populateEdges(node, getNestedBundledDeps(pkg), seen)
  return node
}

function populateEdges (node: TreeNode, deps: string[], seen: Map<string, TreeNode>): void {
  for (const dep of deps) {
    const depDir = resolveDependency(dep, node.path)
    if (!depDir) continue
    const depNode = buildBundledTree(depDir, seen)
    node.edgesOut.set(dep, { to: depNode, peer: false, dev: false })
  }
}

function makeNode (pkgDir: string, pkg: Record<string, unknown>, isProjectRoot: boolean): TreeNode {
  const node = {
    path: pkgDir,
    package: pkg,
    isProjectRoot,
    isLink: false,
    edgesOut: new Map<string, Edge>(),
  } as TreeNode
  node.target = node
  return node
}

function getRootBundledDeps (pkg: Record<string, unknown>): string[] {
  const bundle = pkg.bundleDependencies ?? pkg.bundledDependencies
  if (Array.isArray(bundle)) return bundle as string[]
  if (bundle === true) {
    return Object.keys((pkg.dependencies ?? {}) as Record<string, string>)
  }
  return []
}

function getNestedBundledDeps (pkg: Record<string, unknown>): string[] {
  const dependencies = (pkg.dependencies ?? {}) as Record<string, string>
  const optionalDependencies = (pkg.optionalDependencies ?? {}) as Record<string, string>
  return [...Object.keys(dependencies), ...Object.keys(optionalDependencies)]
}

function resolveDependency (depName: string, fromDir: string): string | undefined {
  let currentDir = fromDir
  while (true) {
    const candidate = path.join(currentDir, 'node_modules', depName)
    try {
      const stat = fs.statSync(path.join(candidate, 'package.json'))
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

function readPackageJson (dir: string): Record<string, unknown> {
  try {
    return JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'))
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
