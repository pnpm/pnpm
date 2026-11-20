import type { Catalogs } from '@pnpm/catalogs.types'
import { isFilespec, isLocalFilesystemSpecifier } from '@pnpm/resolving.local-resolver'
import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'
import { importerIdOf, projectBinsDigest } from '@pnpm/workspace.state'
import { equals, filter } from 'ramda'

import type { WorkspaceDepsStatusContext } from './checkWorkspaceDepsStatus.js'

export function findWorkspaceStructureIssue (ctx: WorkspaceDepsStatusContext): string | undefined {
  const { allProjects, opts, workspaceState } = ctx
  if (!equals(
    filter(value => value != null, workspaceState.settings.catalogs ?? {}),
    filter(value => value != null, opts.catalogs ?? {})
  )) {
    return 'Catalogs cache outdated'
  }
  if (allProjects.length !== Object.keys(workspaceState.projects).length) {
    return WORKSPACE_STRUCTURE_CHANGED
  }
  if (!allProjects.every(({ rootDir, manifest }) => {
    const prevProject = workspaceState.projects[rootDir]
    return prevProject != null && prevProject.name === manifest.name
  })) {
    return WORKSPACE_STRUCTURE_CHANGED
  }
  const bumpedNames = allProjects
    .filter(({ rootDir, manifest }) => (workspaceState.projects[rootDir].version ?? '0.0.0') !== (manifest.version ?? '0.0.0'))
    .map(({ manifest }) => manifest.name)
  if (bumpedNames.length === 0) return undefined
  const resolvesByVersion = createResolvableProjectsFilter(ctx)
  return bumpedNames.some(resolvesByVersion) ? WORKSPACE_STRUCTURE_CHANGED : undefined
}

const WORKSPACE_STRUCTURE_CHANGED = 'The workspace structure has changed since last install'

/**
 * Whether a dependency may resolve to the workspace project of this name by
 * its version. A version decides only how dependencies resolve to the
 * project, so only these versions are compared. It errs toward `true`: a
 * version it wrongly ignores lets scripts run against a stale `node_modules`.
 */
function createResolvableProjectsFilter ({ allProjects, opts, workspaceState }: WorkspaceDepsStatusContext): (name: string | undefined) => boolean {
  // Under deep linking any transitive dependency may resolve to a project.
  if (opts.linkWorkspacePackages === 'deep') return () => true
  // Otherwise no dependency resolves to a lone project.
  if (allProjects.length === 1) return () => false
  // A pnpmfile hook may add a dependency on any project.
  if (workspaceState.pnpmfiles.length > 0) return () => true
  const dependencySpecs = collectDependencySpecs(allProjects, opts.catalogs)
  // A dependency given by a local path may point at any project under any alias.
  if (dependencySpecs.some(([, spec]) => spec != null && isLocalPath(spec))) return () => true
  const dependedNames = new Set(dependencySpecs.map(([alias, spec]) => dependedName(alias, spec)))
  const settingTexts = collectSettingTexts(opts)
  return (name) => name != null &&
    (dependedNames.has(name) || settingTexts.some((text) => text.includes(name)))
}

const DEPENDENCY_FIELDS = ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies'] as const

/** The `[alias, spec]` pairs of the projects' dependencies and of the catalog entries. */
function collectDependencySpecs (allProjects: WorkspaceDepsStatusContext['allProjects'], catalogs: Catalogs | undefined): Array<[string, string | undefined]> {
  return [
    ...allProjects.flatMap(({ manifest }) => DEPENDENCY_FIELDS.flatMap((field) => Object.entries(manifest[field] ?? {}))),
    ...Object.values(catalogs ?? {}).flatMap((catalog) => Object.entries(catalog ?? {})),
  ]
}

function isLocalPath (spec: string): boolean {
  return isLocalFilesystemSpecifier(spec) || (spec.startsWith('workspace:') && isFilespec(spec.slice('workspace:'.length)))
}

/** The package name a dependency names, through an `npm:` or `workspace:` alias where there is one. */
function dependedName (alias: string, spec: string | undefined): string {
  if (spec == null) return alias
  const workspaceSpec = WorkspaceSpec.parse(spec)
  if (workspaceSpec != null) return workspaceSpec.alias ?? alias
  if (!spec.startsWith('npm:')) return alias
  const target = spec.slice('npm:'.length)
  const versionSeparator = target.lastIndexOf('@')
  return versionSeparator >= 1 ? target.slice(0, versionSeparator) : target
}

/**
 * The selectors and values of the overrides, the package extensions, and the
 * patches. Each may select a project by its version or point a dependency at
 * it, and a substring match on them is the cheapest test that never misses one.
 */
function collectSettingTexts ({ overrides, packageExtensions, patchedDependencies }: WorkspaceDepsStatusContext['opts']): string[] {
  return [
    ...Object.entries(overrides ?? {}).flat(),
    ...Object.entries(packageExtensions ?? {}).flatMap(([selector, extension]) => [
      selector,
      ...[extension.dependencies, extension.optionalDependencies, extension.peerDependencies]
        .flatMap((dependencies) => Object.entries(dependencies ?? {}).flat()),
    ]),
    ...Object.keys(patchedDependencies ?? {}),
  ]
}

/**
 * Whether a project the install covers links a workspace bin declaration
 * that differs from what it was proven installed with. `workspaceBins` is the
 * digest of the current declarations across the workspace. A state without a
 * validation record proves nothing about any project, so it raises no issue
 * and leaves the verdict to the checks that handle such a state.
 */
export function findWorkspaceBinsIssue (
  ctx: WorkspaceDepsStatusContext,
  installCovers: (rootDir: string) => boolean,
  workspaceBins: string | undefined
): string | undefined {
  const { allProjects, workspaceState: { validation } } = ctx
  if (validation == null) return undefined
  const lockfileDir = ctx.opts.lockfileDir ?? ctx.workspaceDir
  if (allProjects.some(({ rootDir, manifest }) => {
    if (!installCovers(rootDir)) return false
    const importerId = importerIdOf(lockfileDir, rootDir)
    return validation.workspaceBins?.[importerId] !== projectBinsDigest(importerId, manifest, workspaceBins)
  })) {
    return 'workspace package bins changed since the last install'
  }
  return undefined
}
