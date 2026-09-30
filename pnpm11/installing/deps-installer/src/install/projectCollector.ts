import type { CatalogResultMatcher } from '@pnpm/catalogs.resolver'
import type { createOverriddenDependencyMatcher, createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { PnpmContext } from '@pnpm/installing.context'
import { getAllDependenciesFromManifest, getAllUniqueSpecs, getSpecFromPackageManifest, guessDependencyType } from '@pnpm/pkg-manifest.utils'
import type { DependencyManifest } from '@pnpm/types'

import type { ImporterToUpdate, InstallDepsMutation, InstallSomeDepsMutation, MutationRun } from './mutationTypes.js'

/**
 * The state shared by the steps that turn each mutated project into the
 * importer the resolution installs.
 */
export interface ProjectCollector {
  run: MutationRun
  projectsToInstall: ImporterToUpdate[]
  installedProjectIds: Set<string>
  overriddenDependencyMatcherFor: ReturnType<typeof createOverriddenDependencyMatcher>
  applyOverrides: ReturnType<typeof createReadPackageHook>
  /** The unique specifiers of the workspace packages, computed on first use. */
  getPreferredSpecs: () => Record<string, string>
}

export type InstallCaseProject = Pick<ImporterToUpdate,
| 'binsDir'
| 'buildIndex'
| 'id'
| 'manifest'
| 'modulesDir'
| 'mutation'
| 'originalManifest'
| 'rootDir'
| 'updatePackageManifest'
> & Pick<InstallDepsMutation, 'update'>

export type InstallSomeProject = Pick<ImporterToUpdate,
| 'binsDir'
| 'buildIndex'
| 'id'
| 'manifest'
| 'modulesDir'
| 'mutation'
| 'originalManifest'
| 'rootDir'
| 'updatePackageManifest'
> & Pick<InstallSomeDepsMutation,
| 'allowNew'
| 'dependencySelectors'
| 'peer'
| 'peerAliases'
| 'targetDependenciesField'
| 'update'
| 'updatePatches'
| 'updateToLatest'
>

export const pickCatalogSpecifier: CatalogResultMatcher<string | undefined> = {
  found: (found) =>
    found.resolution.specifier,
  misconfiguration: () => undefined,
  unused: () => undefined,
}

export function memoizePreferredSpecs (ctx: Pick<PnpmContext, 'workspacePackages'>): () => Record<string, string> {
  let preferredSpecs: Record<string, string> | undefined
  return () => {
    preferredSpecs ??= getAllUniqueSpecs(listWorkspaceManifests(ctx))
    return preferredSpecs
  }
}

function listWorkspaceManifests (ctx: Pick<PnpmContext, 'workspacePackages'>): DependencyManifest[] {
  const manifests = []
  for (const versions of ctx.workspacePackages.values()) {
    for (const { manifest } of versions.values()) {
      manifests.push(manifest)
    }
  }
  return manifests
}

/**
 * The direct dependencies a `packageExtensions` entry, a `readPackage` hook, or an override
 * governs rather than the project: absent from the manifest on disk, declared there under
 * another dependency field, declared there with another specifier, or claimed by an override.
 *
 * An update leaves them as the hook supplies them. Resolving them anew moves a version the
 * project never declared, and writing them to `package.json` hands it a declaration the next
 * install rewrites, which `--frozen-lockfile` then rejects (pnpm/pnpm#14928).
 *
 * `undefined` when nothing is protected: a run that is not an update, or a project whose
 * manifest no hook rewrote.
 */
export function getHookOwnedAliases (
  collector: ProjectCollector,
  project: Pick<InstallCaseProject, 'manifest' | 'originalManifest' | 'update'>
): Set<string> | undefined {
  const originalManifest = project.originalManifest
  if (project.update !== true || originalManifest == null) return undefined
  const isOverriddenDependency = collector.overriddenDependencyMatcherFor?.(project.manifest)
  const effectiveDependencies = getAllDependenciesFromManifest(project.manifest, {
    autoInstallPeers: collector.run.opts.autoInstallPeers,
  })
  return new Set(Object.keys(effectiveDependencies).filter((alias) => {
    const originalDependencyType = guessDependencyType(alias, originalManifest)
    if (originalDependencyType == null) return true
    if (guessDependencyType(alias, project.manifest) !== originalDependencyType) return true
    const originalSpecifier = getSpecFromPackageManifest(originalManifest, alias)
    return effectiveDependencies[alias] !== originalSpecifier ||
      isOverriddenDependency?.(alias, originalSpecifier) === true
  }))
}
