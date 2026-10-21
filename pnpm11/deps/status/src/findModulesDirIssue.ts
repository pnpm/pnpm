import type fs from 'node:fs'
import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { createProjectModulesDirResolver } from '@pnpm/config.reader'
import { refToRelative } from '@pnpm/deps.path'
import {
  getLockfileImporterId,
  type LockfileObject,
  type ProjectSnapshot,
  readCurrentLockfile,
  type ResolvedDependencies,
} from '@pnpm/lockfile.fs'
import {
  DEPENDENCIES_FIELDS,
  type IncludedDependencies,
  type Project,
  type ProjectId,
} from '@pnpm/types'
import { getHoistedProjectModulesDir, type WorkspaceState } from '@pnpm/workspace.state'
import { isEmpty } from 'ramda'

import { dereferenceCatalog, rangeMatches, splitNameAndRange } from './findInjectedWorkspaceDep.js'
import { safeStat } from './safeStat.js'
import type { CheckDepsStatusOptions } from './types.js'
import { readWantedLockfileIn } from './wantedLockfiles.js'

export interface ProjectStats {
  project: Project
  manifestStats: fs.Stats
  modulesDirStats: fs.Stats | undefined
}

export interface ModulesDirCheckContext {
  opts: CheckDepsStatusOptions
  workspaceState: WorkspaceState
  allProjects: Project[]
  workspaceDir: string
  allManifestStats: ProjectStats[]
  hasValidationProof?: boolean
}

/**
 * Describes the first project the command needs installed that has
 * dependencies but no modules directory, or that the current lockfile does
 * not record as installed. `undefined` when there is none.
 */
export async function findModulesDirIssue (ctx: ModulesDirCheckContext): Promise<string | undefined> {
  const { allProjects, opts, workspaceState } = ctx
  // A filtered install legitimately leaves the projects it did not select
  // without a modules directory, so a state that records one can only be
  // held to that requirement for the projects the command being gated
  // selected. Skipping those as well would let a filtered `run` or `exec`
  // select a project the filtered install never materialized and run it
  // without its dependencies (https://github.com/pnpm/pnpm/issues/11865).
  const selectedProjectDirs = workspaceState.filteredInstall
    ? selectProjectDirs(allProjects, {
      ...opts,
      autoInstallPeers: opts.autoInstallPeers !== false && workspaceState.settings.autoInstallPeers !== false,
      include: {
        dependencies: workspaceState.settings.production !== false,
        devDependencies: workspaceState.settings.dev !== false,
        optionalDependencies: workspaceState.settings.optional !== false,
      },
    })
    : undefined
  if (selectedProjectDirs?.size === 0) return undefined
  const projectWithoutModulesDir = await findProjectWithoutModulesDir(ctx, selectedProjectDirs)
  if (projectWithoutModulesDir != null) return missingModulesDirIssue(projectWithoutModulesDir)
  const missingRecordedModulesDir = opts.nodeLinker === 'hoisted'
    ? await findProjectMissingRecordedHoistedModulesDir(allProjects, workspaceState, { rootProjectManifestDir: opts.rootProjectManifestDir, selectedProjectDirs })
    : undefined
  if (missingRecordedModulesDir != null) return missingModulesDirIssue(missingRecordedModulesDir)
  if (selectedProjectDirs == null || ctx.hasValidationProof) return undefined
  const notInstalled = await findProjectMissingFromCurrentLockfile(
    allProjects.filter(project => selectedProjectDirs.has(path.resolve(project.rootDir)) && declaresDependencies(project)),
    createCurrentLockfileLocator({ ...opts, workspaceDir: ctx.workspaceDir })
  )
  return notInstalled == null
    ? undefined
    : notInstalledIssue(notInstalled)
}

async function findProjectWithoutModulesDir (
  { allManifestStats, opts, workspaceDir }: ModulesDirCheckContext,
  selectedProjectDirs: Set<string> | undefined
): Promise<Project | undefined> {
  const { rootProjectManifestDir } = opts
  const withoutModulesDir = listProjectsWithoutModulesDir(allManifestStats, selectedProjectDirs, opts.autoInstallPeers)
  // Under dedupeDirectDeps a project whose direct dependencies resolve to
  // the root's targets gets nothing linked, so the linker never creates
  // its modules directory; it is installed all the same.
  const rootModulesDirExists = allManifestStats.some(({ modulesDirStats, project }) =>
    modulesDirStats?.isDirectory() === true && project.rootDir === rootProjectManifestDir)
  const dedupeLockfileDir = opts.lockfileDir ?? workspaceDir ?? rootProjectManifestDir
  const mayBeDeduped = (project: Project): boolean =>
    opts.dedupeDirectDeps === true && rootModulesDirExists && project.rootDir !== rootProjectManifestDir
  const wantedLockfileForDedupe = withoutModulesDir.some((project) => mayBeDeduped(project))
    ? await readWantedLockfileIn(dedupeLockfileDir, opts)
    : null
  return withoutModulesDir.find((project) => !(
    wantedLockfileForDedupe != null &&
    mayBeDeduped(project) &&
    dedupeLinksNothing(wantedLockfileForDedupe, {
      lockfileDir: dedupeLockfileDir,
      rootDir: rootProjectManifestDir,
      projectDir: project.rootDir,
      include: opts.include,
    })
  ))
}

function listProjectsWithoutModulesDir (
  allManifestStats: ProjectStats[],
  selectedProjectDirs: Set<string> | undefined,
  autoInstallPeers?: boolean
): Project[] {
  return allManifestStats
    .filter(({ modulesDirStats, project }) =>
      (selectedProjectDirs == null || selectedProjectDirs.has(path.resolve(project.rootDir))) &&
      modulesDirStats?.isDirectory() !== true && (
        !isEmpty({
          ...project.manifest.dependencies,
          ...project.manifest.devDependencies,
          ...project.manifest.optionalDependencies,
        }) ||
        (autoInstallPeers !== false && hasRequiredPeerDependencies(project))
      ))
    .map(({ project }) => project)
}

function hasRequiredPeerDependencies (project: Project): boolean {
  const peers = project.manifest.peerDependencies
  if (peers == null) return false
  const meta = project.manifest.peerDependenciesMeta
  return Object.keys(peers).some((name) => meta?.[name]?.optional !== true)
}

interface DedupeLinksNothingOptions {
  lockfileDir: string
  rootDir: string
  projectDir: string
  include?: IncludedDependencies
}

/**
 * Whether `dedupeDirectDeps` links nothing into the project at `projectDir`:
 * for every alias the project declares in a materialized group, the wanted
 * lockfile records one target on each side, and the two are the same, which
 * is what the linker compares. An alias declared with differing targets in
 * several groups has one effective target the linker picks by group order;
 * that choice is not reproduced here, so such an alias proves nothing.
 * Neither does a lockfile that lacks either importer.
 */
function dedupeLinksNothing (lockfile: LockfileObject, opts: DedupeLinksNothingOptions): boolean {
  const root = lockfile.importers[getLockfileImporterId(opts.lockfileDir, opts.rootDir)]
  const project = lockfile.importers[getLockfileImporterId(opts.lockfileDir, opts.projectDir)]
  if (root == null || project == null) return false
  const aliases = new Set(materializedGroups(project, opts.include).flatMap((deps) => Object.keys(deps ?? {})))
  return [...aliases].every((alias) => {
    const version = soleTarget(project, alias, opts.include)
    const rootVersion = soleTarget(root, alias, opts.include)
    return version != null && rootVersion != null && resolvesToSameTarget(opts.rootDir, rootVersion, opts.projectDir, version)
  })
}

function materializedGroups (importer: ProjectSnapshot, include: IncludedDependencies | undefined): Array<ResolvedDependencies | undefined> {
  return [
    include?.dependencies === false ? undefined : importer.dependencies,
    include?.devDependencies === false ? undefined : importer.devDependencies,
    include?.optionalDependencies === false ? undefined : importer.optionalDependencies,
  ]
}

function soleTarget (importer: ProjectSnapshot, alias: string, include: IncludedDependencies | undefined): string | undefined {
  const versions = materializedGroups(importer, include)
    .map((deps) => deps?.[alias])
    .filter((version): version is string => version != null)
  return versions.length > 0 && versions.every((version) => version === versions[0]) ? versions[0] : undefined
}

/**
 * Whether two importer dependency versions resolve to one target: the same
 * snapshot, or `link:` paths that name the same directory once resolved
 * against their own importer directories.
 */
function resolvesToSameTarget (rootDir: string, rootVersion: string, projectDir: string, version: string): boolean {
  const rootLink = rootVersion.startsWith('link:')
  const projectLink = version.startsWith('link:')
  if (rootLink !== projectLink) return false
  if (!rootLink) return rootVersion === version
  return path.resolve(rootDir, rootVersion.slice('link:'.length)) === path.resolve(projectDir, version.slice('link:'.length))
}

function missingModulesDirIssue (project: Project): string {
  const id = project.manifest.name ?? project.rootDir
  return `Workspace package ${id} has dependencies but does not have a modules directory`
}

export function declaresDependencies (project: Project): boolean {
  return DEPENDENCIES_FIELDS.some((field) => !isEmpty(project.manifest[field] ?? {}))
}

interface CurrentLockfileLocation {
  virtualStoreDir: string
  importerId: ProjectId
}

/**
 * Where the install that last materialized `project` wrote the current
 * lockfile, and the importer id it has there. The current lockfile always
 * lives in the `.pnpm` directory of the root modules directory, whatever
 * `virtualStoreDir` says.
 */
function createCurrentLockfileLocator (
  opts: Pick<CheckDepsStatusOptions, 'lockfileDir' | 'modulesDir' | 'packageConfigs' | 'sharedWorkspaceLockfile'> & { workspaceDir: string }
): (project: Project) => CurrentLockfileLocation {
  if (opts.sharedWorkspaceLockfile !== false) {
    const lockfileDir = opts.lockfileDir ?? opts.workspaceDir
    const virtualStoreDir = path.join(path.resolve(lockfileDir, opts.modulesDir ?? 'node_modules'), '.pnpm')
    return (project) => ({ virtualStoreDir, importerId: getLockfileImporterId(lockfileDir, project.rootDir) })
  }
  const modulesDirOf = createProjectModulesDirResolver(opts)
  return (project) => ({
    virtualStoreDir: path.join(path.resolve(project.rootDir, modulesDirOf(project.manifest.name) ?? 'node_modules'), '.pnpm'),
    importerId: '.' as ProjectId,
  })
}

/**
 * The first of `projects` that the current lockfile does not record as
 * installed. After a filtered install, a modules directory does not prove
 * that the install materialized a project. The current lockfile keeps every
 * importer, so a project counts as installed only when the current lockfile
 * records a package for each of its direct dependencies that is not a link.
 */
async function findProjectMissingFromCurrentLockfile (
  projects: Project[],
  locateCurrentLockfile: (project: Project) => CurrentLockfileLocation
): Promise<Project | undefined> {
  const currentLockfiles = new Map<string, Promise<LockfileObject | null>>()
  for (const project of projects) {
    const { virtualStoreDir, importerId } = locateCurrentLockfile(project)
    if (!currentLockfiles.has(virtualStoreDir)) {
      currentLockfiles.set(virtualStoreDir, readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false }))
    }
    // eslint-disable-next-line no-await-in-loop -- the scan stops at the first stale project, so later lockfiles are not read
    const currentLockfile = await currentLockfiles.get(virtualStoreDir)
    if (!isProjectRecordedInCurrentLockfile(currentLockfile, importerId)) {
      return project
    }
  }
  return undefined
}

export function notInstalledIssue (project: Project): string {
  return `Workspace package ${project.manifest.name ?? project.rootDir} has dependencies but was not installed`
}

export function isProjectRecordedInCurrentLockfile (
  currentLockfile: LockfileObject | null | undefined,
  importerId: ProjectId
): boolean {
  const importer = currentLockfile?.importers[importerId]
  return importer != null && directDependenciesRecorded(importer, currentLockfile?.packages ?? {})
}

function directDependenciesRecorded (importer: ProjectSnapshot, packages: NonNullable<LockfileObject['packages']>): boolean {
  return !isEmpty(importer.specifiers) &&
    DEPENDENCIES_FIELDS.every((field) =>
      Object.entries(importer[field] ?? {}).every(([alias, ref]) => {
        const depPath = refToRelative(ref, alias)
        return depPath == null || packages[depPath] != null
      }))
}

export function selectProjectDirs (
  allProjects: Project[],
  opts: Pick<CheckDepsStatusOptions, 'autoInstallPeers' | 'catalogs' | 'dir' | 'include' | 'injectWorkspacePackages' | 'linkWorkspacePackages' | 'selectedProjectsGraph' | 'sharedWorkspaceLockfile'>
): Set<string> {
  if (opts.selectedProjectsGraph != null) {
    return new Set(Object.keys(opts.selectedProjectsGraph).map((dir) => path.resolve(dir)))
  }
  if (opts.dir == null) return new Set()
  return walkCommandWorkspaceDependencies(allProjects, path.resolve(opts.dir), {
    autoInstallPeers: opts.autoInstallPeers !== false,
    catalogs: opts.catalogs,
    include: opts.include,
    linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
    onlyLinkDeps: Boolean(opts.sharedWorkspaceLockfile === false && opts.injectWorkspacePackages),
  })
}

interface WorkspaceWalkContext {
  autoInstallPeers: boolean
  include?: IncludedDependencies
  index: ProjectIndex
  linkWorkspacePackages: boolean
  onlyLinkDeps: boolean
  catalogs?: Catalogs
}

function walkCommandWorkspaceDependencies (
  allProjects: Project[],
  startDir: string,
  opts: Omit<WorkspaceWalkContext, 'index'>
): Set<string> {
  const ctx: WorkspaceWalkContext = { ...opts, index: indexProjects(allProjects) }
  const reached = new Set<string>()
  const stack = [startDir]
  while (stack.length > 0) {
    const dir = stack.pop()!
    if (reached.has(dir)) continue
    reached.add(dir)
    const project = ctx.index.byDir.get(dir)
    if (project == null) continue
    stack.push(...directWorkspaceDependencyDirs(dir, project.manifest, ctx))
  }
  return reached
}

interface ProjectIndex {
  byDir: Map<string, Project>
  byName: Map<string, Project[]>
}

function indexProjects (allProjects: Project[]): ProjectIndex {
  const byDir = new Map<string, Project>()
  const byName = new Map<string, Project[]>()
  for (const project of allProjects) {
    const dir = path.resolve(project.rootDir)
    byDir.set(dir, project)
    const name = project.manifest.name
    if (name == null) continue
    const projects = byName.get(name) ?? []
    projects.push(project)
    byName.set(name, projects)
  }
  return { byDir, byName }
}

const WORKSPACE_WALK_FIELDS = [...DEPENDENCIES_FIELDS, 'peerDependencies'] as const

function isFieldIncluded (field: (typeof WORKSPACE_WALK_FIELDS)[number], ctx: WorkspaceWalkContext): boolean {
  return field === 'peerDependencies' ? ctx.autoInstallPeers : ctx.include?.[field] !== false
}

function directWorkspaceDependencyDirs (
  dir: string,
  manifest: Project['manifest'],
  ctx: WorkspaceWalkContext
): string[] {
  const targets: string[] = []
  for (const field of WORKSPACE_WALK_FIELDS) {
    if (!isFieldIncluded(field, ctx)) continue
    for (const [alias, rawPref] of Object.entries(manifest[field] ?? {})) {
      const pref = dereferenceCatalog(alias, rawPref, ctx.catalogs)
      if (pref != null) {
        targets.push(...specWorkspaceTargetDirs({ dir, alias, pref }, ctx))
      }
    }
  }
  return targets
}

function specWorkspaceTargetDirs (
  { dir, alias, pref }: { dir: string, alias: string, pref: string },
  { index, linkWorkspacePackages, onlyLinkDeps }: WorkspaceWalkContext
): string[] {
  if (pref.startsWith('link:')) {
    return resolveKnownProjectDir(dir, pref.slice('link:'.length), index)
  }
  if (onlyLinkDeps) return []
  if (pref.startsWith('file:')) {
    return resolveKnownProjectDir(dir, pref.slice('file:'.length), index)
  }
  const isWorkspaceSpec = pref.startsWith('workspace:')
  if (!isWorkspaceSpec && !linkWorkspacePackages) return []
  const rawSpec = isWorkspaceSpec ? pref.slice('workspace:'.length) : pref
  if (isWorkspaceSpec && /^\.{1,2}(?:[\\/]|$)/.test(rawSpec)) {
    return resolveKnownProjectDir(dir, rawSpec, index)
  }
  const { name, range } = splitNameAndRange(alias, rawSpec)
  return (index.byName.get(name) ?? [])
    .filter((project) => rangeMatches(range, project.manifest.version, isWorkspaceSpec))
    .map((project) => path.resolve(project.rootDir))
}

function resolveKnownProjectDir (dir: string, relPath: string, index: WorkspaceWalkContext['index']): string[] {
  const target = path.resolve(dir, relPath.replace(/\\/g, '/'))
  return index.byDir.has(target) ? [target] : []
}

/**
 * The hoisted linker gives a project its own node_modules only for the
 * dependencies it nests there, so a project without one may be fully
 * installed. The last install recorded which projects have one
 * (`hasModulesDir`); this returns the first of them that no longer does.
 * The workspace root is left out: the workspace state is stored in its
 * node_modules, and the missing-directory check before this one covers it.
 */
async function findProjectMissingRecordedHoistedModulesDir (
  allProjects: Project[],
  workspaceState: WorkspaceState,
  { rootProjectManifestDir, selectedProjectDirs }: {
    rootProjectManifestDir: string
    selectedProjectDirs: Set<string> | undefined
  }
): Promise<Project | undefined> {
  const missing = await Promise.all(allProjects.map(async (project) =>
    (selectedProjectDirs == null || selectedProjectDirs.has(path.resolve(project.rootDir))) &&
    project.rootDir !== rootProjectManifestDir &&
    workspaceState.projects[project.rootDir]?.hasModulesDir === true &&
    (await safeStat(getHoistedProjectModulesDir(project.rootDir)))?.isDirectory() !== true
  ))
  return allProjects.find((_, index) => missing[index])
}
