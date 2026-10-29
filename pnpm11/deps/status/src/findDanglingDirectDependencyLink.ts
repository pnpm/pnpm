import fs from 'node:fs'
import path from 'node:path'

import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { isError } from '@pnpm/error'
import { getLockfileImporterId, type LockfileObject, type ProjectSnapshot, readCurrentLockfile } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

import { readWantedLockfileIn } from './wantedLockfiles.js'

export type FindDanglingDirectDependencyLinkOptions = Pick<Config, 'lockfileDir' | 'modulesDir' | 'nodeLinker' | 'packageConfigs'>
& Partial<Pick<Config, 'dedupeDirectDeps' | 'mergeGitBranchLockfiles' | 'sharedWorkspaceLockfile' | 'symlink' | 'useGitBranchLockfile' | 'workspaceDir'>>
& Pick<ConfigContext, 'allProjects' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'selectedProjectsGraph'>
& {
  include?: IncludedDependencies
  /** Whether a filtered install leaves the workspace root uninstalled. */
  excludeWorkspaceRootProject?: boolean
}

interface DirectDependency {
  alias: string
  spec: string
  rootDir: string
  isRoot: boolean
  /** The entries the linker may place the dependency at, the project's own first. */
  entries: string[]
  /** Whether a missing own entry is checked against the wanted lockfile. */
  mayBeMissingLink: boolean
}

type EntryState = 'present' | 'dangling' | 'missing'

interface Project {
  rootDir: string
  manifest: ProjectManifest
}

interface LinkLayout {
  modulesDirOf: (projectName: string | undefined) => string | undefined
  rootModulesDir: string
  fields: typeof DEPENDENCIES_FIELDS
  /** Whether the linker symlinks every `link:` dependency of the lockfile into `node_modules`. */
  linksLocalDependencies: boolean
}

/**
 * Returns the path of the first direct dependency whose entry in its
 * project's modules directory is a link to a missing target, or a local link
 * the last install made and that is gone. Nothing the up-to-date check reads
 * moves when such a link breaks or is deleted, while the full install
 * relinks it (https://github.com/pnpm/pnpm/issues/9758,
 * https://github.com/pnpm/pnpm/issues/17485).
 *
 * A missing entry counts only for a `workspace:` or `link:` spec of a
 * selected project, only when the linker symlinks direct dependencies into
 * `node_modules`, and only when the lockfile links it
 * ({@link isLockedLinkMissing}). A registry dependency may be absent because
 * it is a skipped optional or excluded one, and a healthy entry costs one
 * `stat`. The hoisted linker places a project's dependencies in the root
 * modules directory too, so both are probed there.
 */
export async function findDanglingDirectDependencyLink (opts: FindDanglingDirectDependencyLinkOptions): Promise<string | undefined> {
  const layout = createLinkLayout(opts)
  const projects = listProjects(opts)
  const dependencies = projects.flatMap((project) => listProjectDependencies(opts, layout, project))
  const states = await Promise.all(dependencies.map(({ entries }) => Promise.all(entries.map(readEntryState))))
  return firstDanglingEntry(dependencies, states) ?? findMissingLockedLink(
    opts,
    { layout, projects, readLockfile: createLockfileReader(opts) },
    dependencies.filter(({ mayBeMissingLink }, index) => mayBeMissingLink && states[index][0] === 'missing')
  )
}

/** The first of the `missing` entries that the lockfile says the last install linked. */
async function findMissingLockedLink (
  opts: FindDanglingDirectDependencyLinkOptions,
  context: LockedLinkContext,
  missing: DirectDependency[]
): Promise<string | undefined> {
  const broken = await Promise.all(missing.map(async (dependency) => isLockedLinkMissing(opts, context, dependency)))
  return missing.find((_, index) => broken[index])?.entries[0]
}

function createLinkLayout (opts: FindDanglingDirectDependencyLinkOptions): LinkLayout {
  const modulesDirOf = createProjectModulesDirResolver(opts)
  return {
    modulesDirOf,
    rootModulesDir: path.resolve(opts.rootProjectManifestDir, modulesDirOf(opts.rootProjectManifest?.name) ?? 'node_modules'),
    fields: DEPENDENCIES_FIELDS.filter((field) => opts.include?.[field] !== false),
    linksLocalDependencies: opts.symlink !== false && (opts.nodeLinker === 'isolated' || opts.nodeLinker === 'hoisted'),
  }
}

function listProjects (opts: FindDanglingDirectDependencyLinkOptions): Project[] {
  const projects: Project[] = [...(opts.allProjects ?? [])]
  if (opts.rootProjectManifest != null && !projects.some(({ rootDir }) => rootDir === opts.rootProjectManifestDir)) {
    projects.push({ rootDir: opts.rootProjectManifestDir, manifest: opts.rootProjectManifest })
  }
  return projects
}

function listProjectDependencies (
  opts: FindDanglingDirectDependencyLinkOptions,
  layout: LinkLayout,
  { rootDir, manifest }: Project
): DirectDependency[] {
  const ownModulesDir = path.resolve(rootDir, layout.modulesDirOf(manifest.name) ?? 'node_modules')
  const isRoot = ownModulesDir === layout.rootModulesDir
  const modulesDirs = opts.nodeLinker === 'hoisted' && !isRoot ? [ownModulesDir, layout.rootModulesDir] : [ownModulesDir]
  const held = layout.linksLocalDependencies && isHeld(opts, rootDir)
  return layout.fields.flatMap((field) => Object.entries(manifest[field] ?? {})).map(([alias, spec]) => ({
    alias,
    spec,
    rootDir,
    isRoot,
    entries: modulesDirs.map((modulesDir) => path.join(modulesDir, alias)),
    mayBeMissingLink: held && isLocalLinkSpec(spec),
  }))
}

/**
 * Whether the install links `rootDir`'s dependencies: every selected project,
 * and the workspace root a shared-lockfile install covers when the selection
 * leaves it out.
 */
function isHeld (opts: FindDanglingDirectDependencyLinkOptions, rootDir: string): boolean {
  if (opts.selectedProjectsGraph == null || rootDir in opts.selectedProjectsGraph) return true
  return rootDir === (opts.workspaceDir ?? opts.rootProjectManifestDir) &&
    opts.sharedWorkspaceLockfile !== false &&
    opts.excludeWorkspaceRootProject !== true
}

function firstDanglingEntry (dependencies: DirectDependency[], states: EntryState[][]): string | undefined {
  for (const [index, { entries }] of dependencies.entries()) {
    const danglingIndex = states[index].indexOf('dangling')
    if (danglingIndex !== -1) return entries[danglingIndex]
  }
  return undefined
}

function isLocalLinkSpec (spec: string): boolean {
  return spec.startsWith('workspace:') || spec.startsWith('link:')
}

interface LockedLinkContext {
  layout: LinkLayout
  projects: Project[]
  readLockfile: (dependency: DirectDependency) => Promise<LockedImporters | undefined>
}

interface LockedImporters {
  /** The importer of the dependency's project. */
  importer: ProjectSnapshot | undefined
  /** The workspace root's importer, when the lockfile is shared. */
  rootImporter: ProjectSnapshot | undefined
}

/**
 * Whether the lockfile links the missing `dependency` to a local directory
 * that no entry the linker may have placed links. The lockfile, not the
 * manifest, says what the last install linked: an override or a
 * `readPackage` hook may drop or rewrite the declared spec, and an injected
 * workspace dependency resolves to a `file:` snapshot that a platform check
 * may skip. It is read only once no root entry links the target the spec
 * names.
 *
 * The hoisted linker may place a project's dependency in the root modules
 * directory, and under `dedupeDirectDeps` a root link to the same target,
 * under any alias, stands in for a project's.
 */
async function isLockedLinkMissing (
  opts: FindDanglingDirectDependencyLinkOptions,
  context: LockedLinkContext,
  dependency: DirectDependency
): Promise<boolean> {
  const sharesRoot = !dependency.isRoot && (opts.nodeLinker === 'hoisted' || opts.dedupeDirectDeps === true)
  const rootEntry = path.join(context.layout.rootModulesDir, dependency.alias)
  const declaredTarget = declaredLinkTarget(context.projects, dependency)
  if (sharesRoot && declaredTarget != null && await resolvesTo(rootEntry, declaredTarget)) return false
  const locked = await context.readLockfile(dependency)
  const target = lockedLinkTarget(opts, locked?.importer, dependency)
  if (target == null) return false
  if (!sharesRoot) return true
  if (await resolvesTo(rootEntry, target)) return false
  return opts.dedupeDirectDeps !== true || !(await rootLinksTarget(opts, context.layout, locked?.rootImporter, target))
}

/** The directory `spec` names: a `link:` path, or the workspace project a `workspace:` spec selects by name. */
function declaredLinkTarget (projects: Project[], { alias, spec, rootDir }: DirectDependency): string | undefined {
  if (spec.startsWith('link:')) return path.resolve(rootDir, spec.slice('link:'.length))
  const range = spec.slice('workspace:'.length)
  const at = range.lastIndexOf('@')
  const name = at > 0 ? range.slice(0, at) : alias
  return projects.find(({ manifest }) => manifest.name === name)?.rootDir
}

function lockedLinkTarget (
  opts: FindDanglingDirectDependencyLinkOptions,
  importer: LockedImporters['importer'],
  { alias, rootDir }: DirectDependency
): string | undefined {
  if (importer == null) return undefined
  const version = DEPENDENCIES_FIELDS
    .filter((field) => opts.include?.[field] !== false)
    .map((field) => importer[field]?.[alias])
    .find((version) => version != null)
  return version?.startsWith('link:') ? path.resolve(rootDir, version.slice('link:'.length)) : undefined
}

/** Whether a root entry the root importer links, under any alias, resolves to `target`. */
async function rootLinksTarget (
  opts: FindDanglingDirectDependencyLinkOptions,
  layout: LinkLayout,
  rootImporter: LockedImporters['importer'],
  target: string
): Promise<boolean> {
  if (rootImporter == null) return false
  const rootDir = opts.workspaceDir ?? opts.rootProjectManifestDir
  const aliases = DEPENDENCIES_FIELDS
    .flatMap((field) => Object.entries<string>(rootImporter[field] ?? {}))
    .filter(([, version]) => version.startsWith('link:') && path.resolve(rootDir, version.slice('link:'.length)) === target)
    .map(([alias]) => alias)
  const linked = await Promise.all(aliases.map(async (alias) => resolvesTo(path.join(layout.rootModulesDir, alias), target)))
  return linked.includes(true)
}

/**
 * Reads each lockfile once: the shared wanted lockfile, or a project's own
 * under `sharedWorkspaceLockfile: false`. The current lockfile stands in for
 * a missing wanted one.
 */
function createLockfileReader (opts: FindDanglingDirectDependencyLinkOptions): (dependency: DirectDependency) => Promise<LockedImporters | undefined> {
  const lockfiles = new Map<string, Promise<LockfileObject | null>>()
  const read = (lockfileDir: string): Promise<LockfileObject | null> => {
    let lockfile = lockfiles.get(lockfileDir)
    if (lockfile == null) {
      lockfile = readWantedOrCurrentLockfile(opts, lockfileDir)
      lockfiles.set(lockfileDir, lockfile)
    }
    return lockfile
  }
  const sharedLockfileDir = opts.lockfileDir ?? opts.rootProjectManifestDir
  return async ({ rootDir, isRoot }) => {
    const shared = opts.sharedWorkspaceLockfile !== false || isRoot
    const lockfileDir = shared ? sharedLockfileDir : rootDir
    const lockfile = await read(lockfileDir)
    if (lockfile == null) return undefined
    return {
      importer: lockfile.importers[getLockfileImporterId(lockfileDir, rootDir)],
      rootImporter: shared ? lockfile.importers[getLockfileImporterId(lockfileDir, opts.workspaceDir ?? opts.rootProjectManifestDir)] : undefined,
    }
  }
}

async function readWantedOrCurrentLockfile (opts: FindDanglingDirectDependencyLinkOptions, lockfileDir: string): Promise<LockfileObject | null> {
  return await readWantedLockfileIn(lockfileDir, opts) ??
    readCurrentLockfile(path.join(lockfileDir, opts.modulesDir ?? 'node_modules', '.pnpm'), { ignoreIncompatible: false })
}

async function resolvesTo (entry: string, target: string): Promise<boolean> {
  const [realEntry, realTarget] = await Promise.all([realpathOrUndefined(entry), realpathOrUndefined(target)])
  return realEntry != null && realEntry === realTarget
}

async function realpathOrUndefined (entry: string): Promise<string | undefined> {
  try {
    return await fs.promises.realpath(entry)
  } catch (error) {
    if (hasErrorCode(error, 'ENOENT', 'ENOTDIR', 'ELOOP')) return undefined
    throw error
  }
}

async function readEntryState (entry: string): Promise<EntryState> {
  try {
    await fs.promises.stat(entry)
    return 'present'
  } catch (error) {
    if (!hasErrorCode(error, 'ENOENT', 'ENOTDIR', 'ELOOP')) throw error
  }
  try {
    await fs.promises.lstat(entry)
    return 'dangling'
  } catch (error) {
    if (hasErrorCode(error, 'ENOENT', 'ENOTDIR')) return 'missing'
    throw error
  }
}

function hasErrorCode (error: unknown, ...codes: string[]): boolean {
  return isError(error) && 'code' in error && codes.includes(String(error.code))
}
