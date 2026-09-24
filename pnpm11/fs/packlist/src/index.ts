import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { isSubdir } from 'is-subdir'
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

const ALTERNATE_MANIFEST_NAMES = ['package.yaml', 'package.json5']

/**
 * npm-packlist applies the manifest rules (`files`, `main`, `bin`, and the
 * always-included files) only when it finds `package.json` on disk. This walker
 * also applies them to a package root that has only a `package.yaml` or
 * `package.json5` manifest, and always includes those manifests like
 * `package.json`.
 *
 * npm-packlist also drops every symlink. This walker keeps them, including
 * symlinks named in `files`, and leaves it to packlistWithSources to exclude the
 * ones that point outside the package.
 */
class PackWalker extends npmPacklist.Walker {
  override onReaddir (entries: string[]): void {
    if (
      this.isPackage &&
      !entries.includes('package.json') &&
      entries.some((entry) => ALTERNATE_MANIFEST_NAMES.includes(entry))
    ) {
      this.processPackage(() => {
        super.onReaddir(entries)
      })
      return
    }
    super.onReaddir(entries)
  }

  override injectRules (filename: string | symbol, rules: string[], callback?: () => void): void {
    if (rules.includes('!/package.json')) {
      rules = [...this.symlinkedFilesRules(), ...rules, ...ALTERNATE_MANIFEST_NAMES.map((name) => `!/${name}`)]
    }
    super.injectRules(filename, rules, callback)
  }

  override onstat (opts: npmPacklist.StatOptions, callback: () => void): void {
    if (opts.st.isSymbolicLink()) {
      ignoreWalkOnstat.call(this, opts, callback)
      return
    }
    super.onstat(opts, callback)
  }

  // npm-packlist makes a `files` entry strictly required only when it is a
  // regular file, so a symlink it names would otherwise be left out.
  private symlinkedFilesRules (): string[] {
    const files = this.tree.package.files
    if (!Array.isArray(files)) return []
    const rules: string[] = []
    for (let file of files as string[]) {
      if (file.startsWith('!')) continue
      if (file.startsWith('./')) file = file.slice(1)
      let stat: fs.Stats
      try {
        stat = fs.lstatSync(path.join(this.path, file))
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && (err.code === 'ENOENT' || err.code === 'ENOTDIR')) continue
        throw err
      }
      if (stat.isSymbolicLink()) {
        rules.push(`!${file}`)
        this.requiredFiles.push(file.startsWith('/') ? file.slice(1) : file)
      }
    }
    return rules
  }
}

// ignore-walk's onstat, which npm-packlist overrides to skip everything that
// is neither a file nor a directory.
const ignoreWalkOnstat = (Object.getPrototypeOf(npmPacklist.Walker.prototype) as npmPacklist.Walker).onstat

interface PlacedPackage {
  // Package names from the packed root down to this package.
  packed: string[]
  realDir: string
  node: TreeNode
}

interface BundleWalk {
  pkgDir: string
  realPkgDir: string
  boundary: string
  rootDependencyNames: Set<string>
  slots: Map<string, string>
  nodes: Map<string, TreeNode>
  packedDirs: Map<TreeNode, string[]>
}

export interface PacklistOptions {
  manifest?: Record<string, unknown>
  workspaceDir?: string
  // The project directory whose node_modules holds the bundled dependencies
  // when pkgDir is a subdirectory of it, such as publishConfig.directory.
  bundledDependenciesDir?: string
}

/**
 * The files that live at their packed path under pkgDir. A bundled dependency
 * resolved through an isolated node_modules layout is packed at a different
 * path than it is read from; packlistWithSources returns those too.
 */
export async function packlist (pkgDir: string, opts?: PacklistOptions): Promise<string[]> {
  const resolvedPkgDir = path.resolve(pkgDir)
  const files = await packlistWithSources(resolvedPkgDir, opts)
  return Array.from(files)
    .filter(([file, source]) => source === path.join(resolvedPkgDir, file))
    .map(([file]) => file)
}

/**
 * Maps each packed path to the file it is read from. Bundled dependencies
 * resolve from pkgDir upward, and never above the workspace root when pkgDir
 * is a workspace package, or above bundledDependenciesDir (default pkgDir)
 * otherwise.
 */
export async function packlistWithSources (pkgDir: string, opts?: PacklistOptions): Promise<Map<string, string>> {
  const resolvedPkgDir = path.resolve(pkgDir)
  const workspaceDir = opts?.workspaceDir == null ? undefined : path.resolve(opts.workspaceDir)
  const pkg = opts?.manifest ?? await safeReadProjectManifestOnly(resolvedPkgDir) as Record<string, unknown> | null ?? {}
  const hasWorkspaceContext = workspaceDir != null && workspaceDir !== resolvedPkgDir && isSubdir(workspaceDir, resolvedPkgDir)
  const boundary = hasWorkspaceContext ? workspaceDir : path.resolve(opts?.bundledDependenciesDir ?? resolvedPkgDir)
  const { tree, packedDirs } = buildRootTree(resolvedPkgDir, pkg, boundary)
  let hasNpmIgnore = false
  if (hasWorkspaceContext) {
    try {
      hasNpmIgnore = (await fs.promises.stat(path.join(resolvedPkgDir, '.npmignore'))).isFile()
    } catch (err: unknown) {
      if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') throw err
    }
  }
  const packlistOpts = hasWorkspaceContext && !hasNpmIgnore
    ? { prefix: workspaceDir, workspaces: [resolvedPkgDir] }
    : undefined
  const walkedFiles = await new Promise<string[]>((resolve, reject) => {
    new PackWalker(tree, { ...packlistOpts, isPackage: true })
      .on('done', resolve)
      .on('error', reject)
      .start()
  })
  const files = walkedFiles
    .map((file) => file.replace(/^\.[/\\]/, ''))
    .filter((file) => isInternalFileOrSymlink(resolvedPkgDir, file))
  return mapToPackedPaths(resolvedPkgDir, files, packedDirs)
}

function isEscapingRelativePath (rel: string): boolean {
  return rel === '..' || rel.startsWith(`..${path.sep}`) || path.isAbsolute(rel)
}

function isInternalFileOrSymlink (pkgDir: string, relFile: string): boolean {
  const absPath = path.join(pkgDir, relFile)
  let lstat: fs.Stats
  try {
    lstat = fs.lstatSync(absPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
    throw err
  }
  if (!lstat.isSymbolicLink()) {
    return true
  }
  const linkTarget = fs.readlinkSync(absPath)
  const relPosix = process.platform === 'win32' ? relFile.replace(/\\/g, '/') : relFile
  const posixTarget = linkTarget.replace(/\\/g, '/')
  if (path.posix.isAbsolute(posixTarget)) {
    return false
  }
  const normalizedArchive = path.posix.normalize(path.posix.join(path.posix.dirname(relPosix), posixTarget))
  if (normalizedArchive === '..' || normalizedArchive.startsWith('../')) {
    return false
  }
  const resolvedTarget = path.resolve(path.dirname(absPath), linkTarget)
  const relToPkg = path.relative(pkgDir, resolvedTarget)
  if (isEscapingRelativePath(relToPkg)) {
    return false
  }
  try {
    const realTarget = fs.realpathSync(absPath)
    const realPkgDir = fs.realpathSync(pkgDir)
    const relReal = path.relative(realPkgDir, realTarget)
    if (isEscapingRelativePath(relReal)) {
      return false
    }
  } catch (err: unknown) {
    if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
      throw err
    }
  }
  return true
}

function mapToPackedPaths (pkgDir: string, files: string[], packedDirs: Map<TreeNode, string[]>): Map<string, string> {
  const bundleDirs = Array.from(packedDirs, ([node, packed]) => ({
    walked: path.relative(pkgDir, node.path).split(path.sep).join('/'),
    packed,
  })).sort((a, b) => b.walked.length - a.walked.length)
  const result = new Map<string, string>()
  for (const file of files) {
    const source = path.join(pkgDir, file)
    const bundle = bundleDirs.find(({ walked }) => file.startsWith(`${walked}/`))
    if (bundle == null) {
      result.set(file, source)
      continue
    }
    for (const packedDir of bundle.packed) {
      result.set(`${packedDir}${file.slice(bundle.walked.length)}`, source)
    }
  }
  return result
}

/**
 * Mirrors npm-bundled: the root's bundled dependencies, then every
 * dependency and optional dependency of a bundled package. Each dependency
 * resolves the way Node resolves it at runtime, from the parent's real
 * directory. The isolated linker keeps a package's dependencies next to its
 * real directory rather than under the link, so the packed location is chosen
 * separately by packedLocation().
 */
function buildRootTree (pkgDir: string, pkg: Record<string, unknown>, boundary: string): { tree: TreeNode, packedDirs: Map<TreeNode, string[]> } {
  const bundledDeps = getRootBundledDeps(pkg)
  // npm-packlist's gatherBundles() iterates package.bundleDependencies directly,
  // so the field must be an array. Normalize true/undefined to an explicit list.
  const normalizedPkg = normalizePackage(pkg)
  normalizedPkg.bundleDependencies = bundledDeps
  delete normalizedPkg.bundledDependencies
  const root = makeNode(pkgDir, normalizedPkg, true)
  const walk: BundleWalk = {
    pkgDir,
    realPkgDir: fs.realpathSync(pkgDir),
    boundary: realpathOrUndefined(boundary) ?? boundary,
    rootDependencyNames: new Set(getNestedBundledDeps(pkg)),
    slots: new Map(),
    nodes: new Map(),
    packedDirs: new Map(),
  }
  const queue = bundledDeps.map((name) => ({ name, parent: { packed: [], realDir: walk.realPkgDir, node: root } as PlacedPackage, depth: 0 }))
  while (queue.length > 0) {
    const task = queue.shift()!
    if (task.depth > MAX_BUNDLE_DEPTH) continue
    const resolved = resolveDependency(task.name, task.parent.realDir, walk.boundary)
    if (resolved == null) continue
    const packed = packedLocation(walk, task.name, task.parent.packed, resolved.realDir)
    if (packed == null) continue
    let node = walk.nodes.get(resolved.realDir)
    if (node == null) {
      const dir = isSubdir(walk.realPkgDir, resolved.dir) ? path.join(pkgDir, path.relative(walk.realPkgDir, resolved.dir)) : resolved.dir
      node = makeNode(dir, normalizePackage(readPackageJson(dir)), false)
      walk.nodes.set(resolved.realDir, node)
      walk.packedDirs.set(node, [])
    }
    task.parent.node.edgesOut.set(task.name, { to: node, peer: false, dev: false })
    walk.packedDirs.get(node)!.push(packedDir(packed))
    walk.slots.set(packed.join('\0'), resolved.realDir)
    const placed = { packed, realDir: resolved.realDir, node }
    for (const name of getNestedBundledDeps(node.package)) {
      queue.push({ name, parent: placed, depth: task.depth + 1 })
    }
  }
  return { tree: root, packedDirs: walk.packedDirs }
}

const MAX_BUNDLE_DEPTH = 32

/**
 * The packed location for a dependency required from the package packed at
 * parent, so that Node resolves it from parent inside the tarball. Prefers the
 * on-disk location when that already works, then the top-level node_modules,
 * then parent's own node_modules. Undefined when a package at the same real
 * directory is already visible from parent, which also ends dependency
 * cycles, or when no location can make it visible.
 */
function packedLocation (walk: BundleWalk, name: string, parent: string[], realDir: string): string[] | undefined {
  const visibleFreeSlots: string[][] = []
  for (let depth = parent.length; depth >= 0; depth--) {
    const slot = [...parent.slice(0, depth), name]
    const occupant = walk.slots.get(slot.join('\0'))
    if (occupant === realDir) return undefined
    if (occupant != null) break
    visibleFreeSlots.push(slot)
  }
  const isVisibleFreeSlot = (slot: string[]): boolean => visibleFreeSlots.some((free) => free.join('\0') === slot.join('\0'))
  const onDisk = packedNames(walk.realPkgDir, realDir)
  if (onDisk != null && isVisibleFreeSlot(onDisk)) return onDisk
  const mayHoist = parent.length === 0 || !walk.rootDependencyNames.has(name)
  if (mayHoist && isVisibleFreeSlot([name])) return [name]
  return visibleFreeSlots[0]
}

function packedDir (packed: string[]): string {
  return packed.map((name) => `node_modules/${name}`).join('/')
}

function packedNames (pkgDir: string, dir: string): string[] | undefined {
  if (!isSubdir(pkgDir, dir) || pkgDir === dir) return undefined
  const segments = path.relative(pkgDir, dir).split(path.sep)
  const names: string[] = []
  while (segments.length > 0) {
    if (segments.shift() !== 'node_modules') return undefined
    const name = segments.shift()
    if (name == null || name.startsWith('.')) return undefined
    if (name.startsWith('@')) {
      const scopedName = segments.shift()
      if (scopedName == null) return undefined
      names.push(`${name}/${scopedName}`)
    } else {
      names.push(name)
    }
  }
  return names
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

function resolveDependency (depName: string, fromDir: string, boundary: string): { dir: string, realDir: string } | undefined {
  if (!isSafeBundleName(depName)) return undefined
  let currentDir = fromDir
  while (true) {
    if (path.basename(currentDir) !== 'node_modules') {
      const candidate = path.join(currentDir, 'node_modules', depName)
      try {
        const stat = fs.statSync(path.join(candidate, 'package.json'))
        if (stat.isFile()) {
          const realDir = fs.realpathSync(candidate)
          if (realDir !== boundary && !isSubdir(boundary, realDir)) return undefined
          return { dir: candidate, realDir }
        }
      } catch (err: unknown) {
        if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'ENOENT') {
          throw err
        }
      }
    }
    if (currentDir === boundary) return undefined
    const parent = path.dirname(currentDir)
    if (parent === currentDir) return undefined
    currentDir = parent
  }
}

// Bundle names come from package.json, so reject paths before joining them under node_modules.
function isSafeBundleName (name: unknown): name is string {
  if (typeof name !== 'string' || name.includes('\\')) return false
  const parts = name.split('/')
  if (parts.length > 2 || (parts.length === 2 && !parts[0].startsWith('@'))) return false
  return parts.every((part) => part !== '' && part !== '.' && part !== '..')
}

function realpathOrUndefined (dir: string): string | undefined {
  try {
    return fs.realpathSync(dir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
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
