import fs from 'node:fs'

import { isError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'

/** One workspace project, as the lookup records it. */
export interface WorkspacePackageEntry {
  manifest: ProjectManifest
  rootDir?: string
  /**
   * The named-registry prefix of the registry the package publishes to, when
   * the workspace declares one for that registry. A published specifier for
   * the package carries it (`work:1.0.0`), so a consumer records one lockfile
   * key for the package whether it addresses the package through the prefix
   * or through this manifest. Absent, the specifier stays plain.
   */
  registryPrefix?: string
}

export type WorkspacePackageContainer =
  | WorkspacePackageEntry[]
  | Map<string, unknown>
  | Record<string, unknown>

export type WorkspacePackageLookup =
  | WorkspacePackageContainer
  | ((depName: string) => ProjectManifest | undefined)

/**
 * The workspace package a `workspace:` specifier addresses. With `version`,
 * the one project of that name at that version, or the only project of the
 * name when none is at that version; several projects at that version, or
 * several projects of the name and no match, answer nothing, since a guess
 * would send a consumer to another project's registry. Without `version`, the first project of the name with a version,
 * else the first, so that a missing version can be reported. With
 * `dependencyDir`, the project that directory resolves to comes first when
 * several projects carry the name, since they may share a version.
 */
export type WorkspacePackageGetter = (depName: string, version?: string, dependencyDir?: string) => WorkspacePackageEntry | undefined

export function buildWorkspaceManifestGetter (
  workspacePackages?: WorkspacePackageLookup
): WorkspacePackageGetter | undefined {
  if (!workspacePackages) return undefined
  if (typeof workspacePackages === 'function') {
    return (depName: string) => {
      const manifest = workspacePackages(depName)
      return manifest == null ? undefined : { manifest }
    }
  }
  const byName = new Map<string, WorkspacePackageEntry[]>()
  collectWorkspaceEntries(workspacePackages, byName)
  return (depName: string, version?: string, dependencyDir?: string) =>
    selectWorkspacePackage(byName.get(depName), version, dependencyDir)
}

function selectWorkspacePackage (
  projects: WorkspacePackageEntry[] | undefined,
  version: string | undefined,
  dependencyDir: string | undefined
): WorkspacePackageEntry | undefined {
  if (projects == null) return undefined
  if (dependencyDir != null && projects.length > 1) {
    const projectInDir = findProjectInDirectory(projects, dependencyDir)
    if (projectInDir != null) return projectInDir
  }
  if (version == null) {
    return projects.find((project) => project.manifest.name && project.manifest.version) ?? projects[0]
  }
  const atVersion = projects.filter((project) => project.manifest.version === version)
  if (atVersion.length === 1) return atVersion[0]
  return atVersion.length === 0 && projects.length === 1 ? projects[0] : undefined
}

/** The project whose directory `dependencyDir` resolves to, through a `node_modules` symlink or a relative path. */
function findProjectInDirectory (
  projects: WorkspacePackageEntry[],
  dependencyDir: string
): WorkspacePackageEntry | undefined {
  const target = realpathIfExists(dependencyDir)
  if (target == null) return undefined
  return projects.find((project) => project.rootDir != null && resolveProjectDir(project.rootDir) === target)
}

/**
 * A project directory that cannot be resolved, for any filesystem reason, is
 * not the directory the dependency was just read from.
 */
function resolveProjectDir (rootDir: string): string | undefined {
  try {
    return fs.realpathSync(rootDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err) return undefined
    throw err
  }
}

function realpathIfExists (dir: string): string | undefined {
  try {
    return fs.realpathSync(dir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return undefined
    throw err
  }
}

function collectWorkspaceEntries (
  workspacePackages: WorkspacePackageContainer,
  byName: Map<string, WorkspacePackageEntry[]>
): void {
  const add = (val: unknown) => addEntry(val, byName)

  if (Array.isArray(workspacePackages)) {
    for (const pkg of workspacePackages) add(pkg)
    return
  }
  if (workspacePackages instanceof Map) {
    collectFromMap(workspacePackages, add)
    return
  }
  for (const val of Object.values(workspacePackages)) add(val)
}

function collectFromMap (map: Map<string, unknown>, add: (val: unknown) => void): void {
  for (const val of map.values()) {
    if (val instanceof Map) {
      for (const subVal of val.values()) add(subVal)
    } else {
      add(val)
    }
  }
}

function addEntry (val: unknown, byName: Map<string, WorkspacePackageEntry[]>): void {
  const entry = extractEntry(val)
  if (!entry?.manifest.name) return
  const projects = byName.get(entry.manifest.name)
  if (projects == null) {
    byName.set(entry.manifest.name, [entry])
  } else {
    projects.push(entry)
  }
}

function extractEntry (val: unknown): WorkspacePackageEntry | undefined {
  if (!val || typeof val !== 'object') return undefined
  if ('manifest' in val && val.manifest && typeof val.manifest === 'object') {
    return entryFromProject(val as { manifest: ProjectManifest, rootDir?: unknown, registryPrefix?: unknown })
  }
  const manifest = extractManifest(val)
  return manifest == null ? undefined : { manifest }
}

/** A project record, with only its string-valued fields kept. */
function entryFromProject (
  { manifest, rootDir, registryPrefix }: { manifest: ProjectManifest, rootDir?: unknown, registryPrefix?: unknown }
): WorkspacePackageEntry {
  return {
    manifest,
    rootDir: typeof rootDir === 'string' ? rootDir : undefined,
    registryPrefix: typeof registryPrefix === 'string' ? registryPrefix : undefined,
  }
}

function extractManifest (val: object): ProjectManifest | undefined {
  if ('package' in val && val.package && typeof val.package === 'object' && 'manifest' in val.package && val.package.manifest && typeof val.package.manifest === 'object') {
    return val.package.manifest as ProjectManifest
  }
  if ('name' in val && typeof (val as Record<string, unknown>).name === 'string') {
    return val as ProjectManifest
  }
  return undefined
}
