import fs from 'node:fs'
import path from 'node:path'

import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { isError } from '@pnpm/error'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

export type FindDanglingDirectDependencyLinkOptions = Pick<Config, 'lockfileDir' | 'modulesDir' | 'nodeLinker' | 'packageConfigs'>
& Partial<Pick<Config, 'dedupeDirectDeps' | 'symlink'>>
& Pick<ConfigContext, 'allProjects' | 'rootProjectManifest' | 'rootProjectManifestDir' | 'selectedProjectsGraph'>
& {
  include?: IncludedDependencies
}

interface DirectDependency {
  /** The entries the linker may place the dependency at. */
  entries: string[]
  /** Under `dedupeDirectDeps`, the root's entry that may stand in for a project's. */
  dedupedEntry?: string
  /** Whether the dependency is broken when no entry exists. */
  mustBeLinked: boolean
}

type EntryState = 'present' | 'dangling' | 'missing'

/**
 * Returns the path of the first direct dependency whose entry in its
 * project's modules directory is a link to a missing target, or is a missing
 * `workspace:` or `link:` dependency. Nothing the up-to-date check reads
 * moves when such a link breaks or is deleted, while the full install
 * relinks it (https://github.com/pnpm/pnpm/issues/9758,
 * https://github.com/pnpm/pnpm/issues/17485).
 *
 * A missing entry counts only for a `workspace:` or `link:` spec of a
 * selected project, and only when the linker symlinks direct dependencies
 * into `node_modules`. A registry dependency may be absent because it is a
 * skipped optional or excluded one, and a healthy entry costs one `stat`.
 * The hoisted linker places a project's dependencies in the root modules
 * directory too, so both are probed there. Under `dedupeDirectDeps` a
 * project's dependency that resolves to the root's target is linked only
 * into the root's modules directory.
 */
export async function findDanglingDirectDependencyLink (opts: FindDanglingDirectDependencyLinkOptions): Promise<string | undefined> {
  const dependencies = listDirectDependencies(opts)
  const broken = await Promise.all(dependencies.map(isBroken))
  return dependencies.find((_, index) => broken[index])?.entries[0]
}

interface Project {
  rootDir: string
  manifest: ProjectManifest
}

interface LinkLayout {
  modulesDirOf: (projectName: string | undefined) => string | undefined
  rootModulesDir: string
  fields: typeof DEPENDENCIES_FIELDS
  /** Whether the linker symlinks every `workspace:` and `link:` dependency into `node_modules`. */
  linksLocalDependencies: boolean
}

function listDirectDependencies (opts: FindDanglingDirectDependencyLinkOptions): DirectDependency[] {
  const modulesDirOf = createProjectModulesDirResolver(opts)
  const layout: LinkLayout = {
    modulesDirOf,
    rootModulesDir: path.resolve(opts.rootProjectManifestDir, modulesDirOf(opts.rootProjectManifest?.name) ?? 'node_modules'),
    fields: DEPENDENCIES_FIELDS.filter((field) => opts.include?.[field] !== false),
    linksLocalDependencies: opts.symlink !== false && (opts.nodeLinker === 'isolated' || opts.nodeLinker === 'hoisted'),
  }
  return listProjects(opts).flatMap((project) => listProjectDependencies(opts, layout, project))
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
  const held = layout.linksLocalDependencies && (opts.selectedProjectsGraph == null || rootDir in opts.selectedProjectsGraph)
  const dedupesIntoRoot = opts.dedupeDirectDeps === true && !isRoot
  return layout.fields.flatMap((field) => Object.entries(manifest[field] ?? {})).map(([alias, spec]) => {
    const mustBeLinked = held && isLocalLinkSpec(spec)
    return {
      entries: modulesDirs.map((modulesDir) => path.join(modulesDir, alias)),
      dedupedEntry: mustBeLinked && dedupesIntoRoot ? path.join(layout.rootModulesDir, alias) : undefined,
      mustBeLinked,
    }
  })
}

function isLocalLinkSpec (spec: string): boolean {
  return spec.startsWith('workspace:') || spec.startsWith('link:')
}

async function isBroken ({ entries, dedupedEntry, mustBeLinked }: DirectDependency): Promise<boolean> {
  const states = await Promise.all(entries.map(readEntryState))
  if (states.includes('dangling')) return true
  if (!mustBeLinked || states.includes('present')) return false
  return dedupedEntry == null || await readEntryState(dedupedEntry) === 'missing'
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
