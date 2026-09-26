import * as dp from '@pnpm/deps.path'
import type { ChangedSettingsField } from '@pnpm/lockfile.settings-checker'
import type { LockfileObject, LockfileSettings, ProjectSnapshot } from '@pnpm/lockfile.types'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DepPath, ProjectId, ProjectManifest } from '@pnpm/types'

interface Project {
  id: ProjectId
  manifest: ProjectManifest
}

export interface FastSettingsUpdateOptions {
  changedSettings: ChangedSettingsField[]
  projects: Project[]
  settings: LockfileSettings
  workspacePackages: WorkspacePackages
}

/**
 * Record a changed lockfile setting without resolving the dependency graph,
 * for the settings that the lockfile itself proves cannot affect its graph:
 * peer settings when nothing declares a peer dependency, and the link and
 * injection settings when no project depends on a directory or on another
 * workspace project.
 *
 * Returns `false` for everything else, which keeps the full-resolution path
 * as the fallback.
 */
export function tryFastUpdateSettings (
  lockfile: LockfileObject,
  opts: FastSettingsUpdateOptions
): boolean {
  if (opts.changedSettings.length === 0) return false
  if (!opts.changedSettings.every((setting) => settingCannotAffectLockfile(setting, lockfile, opts))) {
    return false
  }
  lockfile.settings = { ...opts.settings }
  return true
}

function settingCannotAffectLockfile (
  setting: ChangedSettingsField,
  lockfile: LockfileObject,
  opts: FastSettingsUpdateOptions
): boolean {
  switch (setting) {
    case 'settings.autoInstallPeers':
    case 'settings.dedupePeers':
    case 'settings.peersSuffixMaxLength':
      return hasNoPeerDependencies(lockfile, opts.projects)
    case 'settings.excludeLinksFromLockfile':
      return hasNoLinkedDependencies(lockfile, opts.projects, opts.workspacePackages)
    case 'settings.injectWorkspacePackages':
      return hasNoInjectableDependencies(lockfile, opts.projects, opts.workspacePackages)
  }
}

/**
 * All three peer settings only change how peer dependencies are resolved,
 * deduplicated, and hashed into dep path suffixes. None of them has anything
 * to act on when no package or project declares a peer dependency and no dep
 * path carries a peers suffix.
 */
function hasNoPeerDependencies (lockfile: LockfileObject, projects: Project[]): boolean {
  for (const [depPath, pkg] of Object.entries(lockfile.packages ?? {})) {
    if (dp.parseDepPath(depPath as DepPath).peerDepGraphHash !== '') return false
    if (!isEmpty(pkg.peerDependencies) || !isEmpty(pkg.peerDependenciesMeta)) return false
    if (pkg.transitivePeerDependencies?.length) return false
  }
  return projects.every((project) => isEmpty(project.manifest.peerDependencies))
}

/**
 * `excludeLinksFromLockfile` decides whether a dependency that resolves to a
 * directory is recorded in the lockfile. Dependencies declared with the
 * `workspace:` protocol are recorded either way, so only the other directory
 * dependencies matter — including plain ranges that `linkWorkspacePackages`
 * turns into links to a workspace project.
 */
function hasNoLinkedDependencies (
  lockfile: LockfileObject,
  projects: Project[],
  workspacePackages: WorkspacePackages
): boolean {
  if (Object.values(lockfile.importers).some(hasDirectoryReference)) return false
  return projects.every((project) =>
    manifestDependencies(project.manifest).every(([alias, bareSpecifier]) =>
      bareSpecifier.startsWith('workspace:') ||
      !isDirectoryDependency(alias, bareSpecifier, workspacePackages)
    )
  )
}

/**
 * `injectWorkspacePackages` replaces the symlinks to workspace projects with
 * hard-linked copies, which the lockfile records as directory dependencies of
 * the importer. An install with no dependency on any workspace project has
 * nothing to inject.
 */
function hasNoInjectableDependencies (
  lockfile: LockfileObject,
  projects: Project[],
  workspacePackages: WorkspacePackages
): boolean {
  if (Object.values(lockfile.importers).some(hasDirectoryReference)) return false
  return projects.every((project) =>
    Object.values(project.manifest.dependenciesMeta ?? {}).every((meta) => !meta.injected) &&
    manifestDependencies(project.manifest).every(([alias, bareSpecifier]) =>
      !isDirectoryDependency(alias, bareSpecifier, workspacePackages)
    )
  )
}

/**
 * Whether `alias` resolves to a directory rather than to a registry version:
 * the protocols that name one outright, and a plain range on a workspace
 * project's name, which `linkWorkspacePackages` turns into a link.
 */
export function isDirectoryDependency (
  alias: string,
  bareSpecifier: string,
  workspacePackages: WorkspacePackages
): boolean {
  return bareSpecifier.startsWith('workspace:') ||
    bareSpecifier.startsWith('link:') ||
    bareSpecifier.startsWith('file:') ||
    workspacePackages.has(alias)
}

function hasDirectoryReference (importer: ProjectSnapshot): boolean {
  return [importer.dependencies, importer.devDependencies, importer.optionalDependencies]
    .some((deps) => Object.values(deps ?? {}).some((ref) =>
      ref.startsWith('link:') || ref.startsWith('file:')
    ))
}

function manifestDependencies (manifest: ProjectManifest): Array<[string, string]> {
  return [
    ...Object.entries(manifest.devDependencies ?? {}),
    ...Object.entries(manifest.dependencies ?? {}),
    ...Object.entries(manifest.optionalDependencies ?? {}),
  ]
}

function isEmpty (record: Record<string, unknown> | undefined): boolean {
  return record == null || Object.keys(record).length === 0
}
