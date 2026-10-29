import fs from 'node:fs'
import path from 'node:path'

import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { isError } from '@pnpm/error'
import { getLockfileImporterId, type LockfileObject } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

import { readWantedLockfileIn } from './wantedLockfiles.js'

export type FindDanglingDirectDependencyLinkOptions = Pick<Config, 'lockfileDir' | 'modulesDir' | 'nodeLinker' | 'packageConfigs'>
& Partial<Pick<Config, 'dedupeDirectDeps' | 'mergeGitBranchLockfiles' | 'symlink' | 'useGitBranchLockfile'>>
& Pick<ConfigContext, 'allProjects' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'selectedProjectsGraph'>
& {
  include?: IncludedDependencies
}

interface DirectDependency {
  alias: string
  rootDir: string
  isRoot: boolean
  /** The entries the linker may place the dependency at, the project's own first. */
  entries: string[]
  /** Whether a missing own entry is checked against the wanted lockfile. */
  mayBeMissingLink: boolean
}

type EntryState = 'present' | 'dangling' | 'missing'

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
 * `node_modules`, and only when the wanted lockfile links it
 * ({@link isLockedLinkMissing}). A registry dependency may be absent because
 * it is a skipped optional or excluded one, and a healthy entry costs one
 * `stat`. The hoisted linker places a project's dependencies in the root
 * modules directory too, so both are probed there.
 */
export async function findDanglingDirectDependencyLink (opts: FindDanglingDirectDependencyLinkOptions): Promise<string | undefined> {
  const layout = createLinkLayout(opts)
  const dependencies = listProjects(opts).flatMap((project) => listProjectDependencies(opts, layout, project))
  const states = await Promise.all(dependencies.map(({ entries }) => Promise.all(entries.map(readEntryState))))
  const dangling = firstDanglingEntry(dependencies, states)
  if (dangling != null) return dangling
  const missing = dependencies.filter(({ mayBeMissingLink }, index) => mayBeMissingLink && states[index][0] === 'missing')
  return missing.length === 0 ? undefined : findMissingLockedLink(opts, layout, missing)
}

/** The first of the `missing` entries that the wanted lockfile says the last install linked. */
async function findMissingLockedLink (
  opts: FindDanglingDirectDependencyLinkOptions,
  layout: LinkLayout,
  missing: DirectDependency[]
): Promise<string | undefined> {
  const lockfileDir = opts.lockfileDir ?? opts.rootProjectManifestDir
  const lockfile = await readWantedLockfileIn(lockfileDir, opts)
  if (lockfile == null) return undefined
  const context: LockedLinkContext = { lockfile, lockfileDir, rootLinkTargets: createRootLinkTargetsReader(layout.rootModulesDir) }
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

function listProjects (opts: FindDanglingDirectDependencyLinkOptions): Array<{ rootDir: string, manifest: ProjectManifest }> {
  const projects: Array<{ rootDir: string, manifest: ProjectManifest }> = [...(opts.allProjects ?? [])]
  if (opts.rootProjectManifest != null && !projects.some(({ rootDir }) => rootDir === opts.rootProjectManifestDir)) {
    projects.push({ rootDir: opts.rootProjectManifestDir, manifest: opts.rootProjectManifest })
  }
  return projects
}

function listProjectDependencies (
  opts: FindDanglingDirectDependencyLinkOptions,
  layout: LinkLayout,
  { rootDir, manifest }: { rootDir: string, manifest: ProjectManifest }
): DirectDependency[] {
  const ownModulesDir = path.resolve(rootDir, layout.modulesDirOf(manifest.name) ?? 'node_modules')
  const isRoot = ownModulesDir === layout.rootModulesDir
  const modulesDirs = opts.nodeLinker === 'hoisted' && !isRoot ? [ownModulesDir, layout.rootModulesDir] : [ownModulesDir]
  const held = layout.linksLocalDependencies && (opts.selectedProjectsGraph == null || rootDir in opts.selectedProjectsGraph)
  return layout.fields.flatMap((field) => Object.entries(manifest[field] ?? {})).map(([alias, spec]) => ({
    alias,
    rootDir,
    isRoot,
    entries: modulesDirs.map((modulesDir) => path.join(modulesDir, alias)),
    mayBeMissingLink: held && isLocalLinkSpec(spec),
  }))
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
  lockfile: LockfileObject
  lockfileDir: string
  rootLinkTargets: () => Promise<Set<string>>
}

/**
 * Whether the wanted lockfile links the missing `dependency` to a local
 * directory that no entry the linker may have placed links. The lockfile, not
 * the manifest, says what the last install linked: an override or a
 * `readPackage` hook may drop or rewrite the declared spec, and an injected
 * workspace dependency resolves to a `file:` snapshot that a platform check
 * may skip.
 *
 * The hoisted linker may place a project's dependency in the root modules
 * directory, and under `dedupeDirectDeps` a root link to the same target,
 * under any alias, stands in for a project's.
 */
async function isLockedLinkMissing (
  opts: FindDanglingDirectDependencyLinkOptions,
  { lockfile, lockfileDir, rootLinkTargets }: LockedLinkContext,
  dependency: DirectDependency
): Promise<boolean> {
  const target = lockedLinkTarget(opts, { lockfile, lockfileDir }, dependency)
  if (target == null) return false
  if (dependency.isRoot) return true
  const realTarget = await realpathOrUndefined(target)
  if (realTarget == null) return false
  if (opts.nodeLinker === 'hoisted' && await realpathOrUndefined(dependency.entries[1]) === realTarget) return false
  return opts.dedupeDirectDeps !== true || !(await rootLinkTargets()).has(realTarget)
}

function lockedLinkTarget (
  opts: FindDanglingDirectDependencyLinkOptions,
  { lockfile, lockfileDir }: Pick<LockedLinkContext, 'lockfile' | 'lockfileDir'>,
  { alias, rootDir }: DirectDependency
): string | undefined {
  const importer = lockfile.importers[getLockfileImporterId(lockfileDir, rootDir)]
  if (importer == null) return undefined
  const version = DEPENDENCIES_FIELDS
    .filter((field) => opts.include?.[field] !== false)
    .map((field) => importer[field]?.[alias])
    .find((version) => version != null)
  return version?.startsWith('link:') ? path.resolve(rootDir, version.slice('link:'.length)) : undefined
}

/** Reads, once, the real paths of the entries in the root modules directory. */
function createRootLinkTargetsReader (rootModulesDir: string): () => Promise<Set<string>> {
  let targets: Promise<Set<string>> | undefined
  return () => {
    targets ??= readLinkTargets(rootModulesDir)
    return targets
  }
}

async function readLinkTargets (modulesDir: string): Promise<Set<string>> {
  const entries = await listModulesDirEntries(modulesDir)
  const targets = await Promise.all(entries.map(realpathOrUndefined))
  return new Set(targets.filter((target) => target != null))
}

async function listModulesDirEntries (modulesDir: string): Promise<string[]> {
  const names = await readdirOrEmpty(modulesDir)
  const nested = await Promise.all(names.map(async (name) => {
    if (name.startsWith('.')) return []
    if (!name.startsWith('@')) return [path.join(modulesDir, name)]
    const scoped = await readdirOrEmpty(path.join(modulesDir, name))
    return scoped.map((bare) => path.join(modulesDir, name, bare))
  }))
  return nested.flat()
}

async function readdirOrEmpty (dir: string): Promise<string[]> {
  try {
    return await fs.promises.readdir(dir)
  } catch (error) {
    if (hasErrorCode(error, 'ENOENT', 'ENOTDIR')) return []
    throw error
  }
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
