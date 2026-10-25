import fs from 'node:fs'
import path from 'node:path'

import { parseJSON5 } from 'confbox/json5'
import { parseYAML } from 'confbox/yaml'
import picomatch from 'picomatch'
import semver from 'semver'

/** The pnpm version a project pins, and where the pin was read from. */
export interface ProjectPin {
  /** An exact version, or a range for `resolveVersion` to resolve. */
  spec: string
  /** The manifest that declares the pin. */
  manifestPath: string
  /** The field of the manifest that declares it. */
  field: 'devEngines.packageManager' | 'packageManager'
  /** The version `pnpm-lock.yaml` records for a range pin, when it is still in range. */
  lockedVersion?: string
}

/** The basenames pnpm reads a project manifest from, in the order it tries them. */
const MANIFEST_NAMES = ['package.json', 'package.json5', 'package.yaml']

/**
 * The pnpm version that the project `dir` belongs to pins, read the way pnpm
 * reads it to switch to that version.
 *
 * - The workspace is the nearest directory up from `dir` with a
 *   `pnpm-workspace.yaml`, and the project the nearest one with a manifest.
 * - The workspace root and the projects its `packages` patterns match read
 *   the pin of the root's manifest. Any other project reads its own.
 * - When that manifest pins nothing, the pin of the other one applies.
 * - `devEngines.packageManager` takes precedence over `packageManager`, which
 *   pnpm reads only as an exact version.
 * - A range pin comes with the version the lockfile already settled on, which
 *   is the one pnpm switches to while it stays in range. For the workspace,
 *   that lockfile is in `lockfileDir`, or else the root. Any other project
 *   reads its own. A pin taken from the other manifest has none.
 *
 * `opts.root` stops the search for both at that directory, such as the root
 * of a checkout, which keeps a workspace that happens to sit above it out.
 *
 * @returns `undefined` when there is no pnpm pin to read: no manifest, or
 * one that pins no package manager or another one.
 * @throws if a manifest pins pnpm in a way pnpm itself would reject, or a
 * manifest or `pnpm-workspace.yaml` cannot be parsed.
 */
export function readProjectPin (dir: string, opts: { root?: string } = {}): ProjectPin | undefined {
  const start = path.resolve(dir)
  const root = opts.root == null ? undefined : path.resolve(opts.root)
  const workspaceDir = findUp(start, root, (candidate) => fs.existsSync(path.join(candidate, 'pnpm-workspace.yaml')))
  const projectDir = findUp(start, root, (candidate) => MANIFEST_NAMES.some((name) => fs.existsSync(path.join(candidate, name))))
  const sources = pinSources(workspaceDir, projectDir)
  const primary = pinOf(readManifest(sources.primary.manifestDir))
  if (primary == null) return pinOf(readManifest(sources.fallbackManifestDir))
  if (semver.valid(primary.spec) != null || sources.primary.lockfileDir == null) return primary
  const lockedVersion = lockedPnpmVersion(sources.primary.lockfileDir, primary.spec)
  return lockedVersion == null ? primary : { ...primary, lockedVersion }
}

interface PinSources {
  primary: { manifestDir?: string, lockfileDir?: string }
  fallbackManifestDir?: string
}

function pinSources (workspaceDir: string | undefined, projectDir: string | undefined): PinSources {
  if (workspaceDir == null) return { primary: { manifestDir: projectDir, lockfileDir: projectDir } }
  const workspace = readWorkspaceManifest(workspaceDir)
  if (projectDir == null || isWorkspaceProject(workspaceDir, workspace, projectDir)) {
    const lockfileDir = typeof workspace.lockfileDir === 'string' ? path.resolve(workspaceDir, workspace.lockfileDir) : workspaceDir
    return { primary: { manifestDir: workspaceDir, lockfileDir }, fallbackManifestDir: projectDir }
  }
  return { primary: { manifestDir: projectDir, lockfileDir: projectDir }, fallbackManifestDir: workspaceDir }
}

/**
 * The top-level settings of `pnpm-workspace.yaml`. Settings that its
 * `include` delegates to other files are not read, as pnpm does not read them
 * before it switches.
 */
function readWorkspaceManifest (workspaceDir: string): Record<string, unknown> {
  const file = path.join(workspaceDir, 'pnpm-workspace.yaml')
  const value = parseYaml(file, fs.readFileSync(file, 'utf8'))
  return isRecord(value) ? value : {}
}

/**
 * Whether `projectDir` is the workspace root or a project its `packages`
 * patterns match. Without `packages`, the root is the only one.
 */
function isWorkspaceProject (workspaceDir: string, workspace: Record<string, unknown>, projectDir: string): boolean {
  const relative = path.relative(workspaceDir, projectDir).split(path.sep).join('/')
  if (relative === '') return true
  if (relative.startsWith('../') || relative.split('/').some((part) => part === 'node_modules' || part === 'bower_components')) return false
  const patterns = Array.isArray(workspace.packages) ? workspace.packages.filter((pattern) => typeof pattern === 'string') : []
  const matches = (pattern: string): boolean => picomatch.isMatch(relative, pattern.replace(/^\.\//, '').replace(/\/$/, ''))
  const excludes = patterns.filter((pattern) => pattern.startsWith('!')).map((pattern) => pattern.slice(1))
  return !excludes.some(matches) && patterns.filter((pattern) => !pattern.startsWith('!')).some(matches)
}

interface Manifest {
  path: string
  value: Record<string, unknown>
}

function readManifest (dir: string | undefined): Manifest | undefined {
  if (dir == null) return undefined
  for (const name of MANIFEST_NAMES) {
    const manifestPath = path.join(dir, name)
    const content = readOptionalFile(manifestPath)
    if (content == null) continue
    const value = parseManifest(manifestPath, content)
    if (!isRecord(value)) throw new Error(`${manifestPath} does not hold an object.`)
    return { path: manifestPath, value }
  }
  return undefined
}

function parseManifest (file: string, content: string): unknown {
  if (file.endsWith('.yaml')) return parseYaml(file, content)
  try {
    // Windows tools write a byte order mark, which pnpm reads past.
    const text = content.replace(/^\uFEFF/, '')
    return file.endsWith('.json5') ? parseJSON5(text) : JSON.parse(text)
  } catch (err) {
    throw new Error(`${file} is not valid ${file.endsWith('.json5') ? 'JSON5' : 'JSON'}: ${(err as Error).message}`, { cause: err })
  }
}

function parseYaml (file: string, content: string): unknown {
  try {
    return parseYAML(content)
  } catch (err) {
    throw new Error(`${file} is not valid YAML: ${(err as Error).message}`, { cause: err })
  }
}

function pinOf (manifest: Manifest | undefined): ProjectPin | undefined {
  const wanted = manifest == null ? undefined : wantedPnpm(manifest)
  return wanted == null ? undefined : { ...wanted, manifestPath: manifest!.path }
}

function wantedPnpm (manifest: Manifest): Pick<ProjectPin, 'spec' | 'field'> | undefined {
  const engine = devEnginesPackageManager(manifest.value)
  if (engine != null) return engine.name === 'pnpm' ? devEnginesPin(manifest, engine.version) : undefined
  const { packageManager } = manifest.value
  return typeof packageManager === 'string' ? packageManagerPin(manifest, packageManager) : undefined
}

function devEnginesPin (manifest: Manifest, version: unknown): Pick<ProjectPin, 'spec' | 'field'> {
  if (typeof version !== 'string' || semver.validRange(version) == null) {
    throw new Error(`${manifest.path} names pnpm in devEngines.packageManager, but no version or range of it.`)
  }
  return { spec: version, field: 'devEngines.packageManager' }
}

function packageManagerPin (manifest: Manifest, packageManager: string): Pick<ProjectPin, 'spec' | 'field'> | undefined {
  // `pnpm@12.1.0+sha512.<hash>`: the build names the artifact Corepack downloaded,
  // not a pnpm release.
  const separator = packageManager.lastIndexOf('@')
  if (separator <= 0 || packageManager.slice(0, separator) !== 'pnpm') return undefined
  const version = packageManager.slice(separator + 1).split('+')[0]!
  if (semver.valid(version) !== version) {
    throw new Error(`${manifest.path} sets packageManager to "${packageManager}", which pins no exact pnpm version.`)
  }
  return { spec: version, field: 'packageManager' }
}

/** The entry that governs pnpm: pnpm's own in a list, or else the first one. */
function devEnginesPackageManager (manifest: Record<string, unknown>): Record<string, unknown> | undefined {
  const declared = isRecord(manifest.devEngines) ? manifest.devEngines.packageManager : undefined
  if (!Array.isArray(declared)) return isRecord(declared) ? declared : undefined
  const entries = declared.filter(isRecord)
  return entries.find((entry) => entry.name === 'pnpm') ?? entries[0]
}

function lockedPnpmVersion (lockfileDir: string, range: string): string | undefined {
  const lockfilePath = path.join(lockfileDir, 'pnpm-lock.yaml')
  const content = readOptionalFile(lockfilePath)
  const envDocument = content == null ? undefined : extractEnvDocument(content)
  if (envDocument == null) return undefined
  const locked = valueAt(parseYaml(lockfilePath, envDocument), ['importers', '.', 'packageManagerDependencies', 'pnpm', 'version'])
  // In range the way pnpm checks a locked version, prereleases included.
  if (typeof locked !== 'string' || semver.valid(locked) == null) return undefined
  return semver.satisfies(locked, range, { includePrerelease: true }) ? locked : undefined
}

/**
 * The document that leads a pnpm 11+ lockfile and records the package
 * manager. Sliced out rather than parsed with the rest, which runs to
 * megabytes in a large workspace.
 */
function extractEnvDocument (content: string): string | undefined {
  const normalized = content.replace(/^\uFEFF/, '').replaceAll('\r\n', '\n')
  if (!normalized.startsWith('---\n')) return undefined
  const end = normalized.indexOf('\n---\n', 4)
  return end === -1 ? normalized.slice(4).replace(/\n---\n?$/, '') : normalized.slice(4, end)
}

function valueAt (value: unknown, keys: string[]): unknown {
  return keys.reduce<unknown>((current, key) => isRecord(current) ? current[key] : undefined, value)
}

export function findUp (start: string, root: string | undefined, matches: (dir: string) => boolean): string | undefined {
  for (let dir = start; ; dir = path.dirname(dir)) {
    if (matches(dir)) return dir
    if (dir === root || path.dirname(dir) === dir) return undefined
  }
}

function readOptionalFile (file: string): string | undefined {
  try {
    return fs.readFileSync(file, 'utf8')
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return undefined
    throw err
  }
}

function isRecord (value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value != null && !Array.isArray(value)
}
