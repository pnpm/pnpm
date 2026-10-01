import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { Dependencies, ProjectManifest, ReadPackageHook } from '@pnpm/types'
import { clone } from 'ramda'

import { mergeInstallSelectors } from './mergeInstallSelectors.js'
import type { InstallSomeDepsMutation } from './mutationTypes.js'
import type { InstallSomeProject, ProjectCollector } from './projectCollector.js'

export interface HookGovernedAdds {
  superseded?: Map<string, string>
  removed?: Set<string>
}

interface ProbedDependencies {
  declared: Dependencies
  overridden: Dependencies | undefined
  probed: Dependencies
}

/**
 * What the `readPackage` hooks do to the aliases an add names, once the requested declarations
 * are in the manifest the next install reads. Saving a declaration the hooks rewrite or delete
 * would leave the manifest and the lockfile disagreeing, which `--frozen-lockfile` then
 * rejects (pnpm/pnpm#15156): the hook's specifier is saved in the first case, and the request
 * is dropped in the second. Overrides are excluded: an explicit version intentionally ignores
 * them.
 *
 * The hooks run over the manifest on disk carrying the requested declarations, rather than
 * over the manifest this run has already put through them, and what they produced is judged
 * against those declarations — which is what a selector naming no version of its own has in
 * place of a request. A difference the overrides alone account for leaves the request
 * standing.
 *
 * `undefined` when every request survives the hooks.
 */
export async function getHookGovernedAdds (
  collector: ProjectCollector,
  project: Pick<InstallSomeProject, 'dependencySelectors' | 'manifest' | 'originalManifest' | 'peer' | 'peerAliases' | 'rootDir' | 'targetDependenciesField'>
): Promise<HookGovernedAdds | undefined> {
  const { opts } = collector.run
  const hooks = opts.readPackageHook == null
    ? []
    : Array.isArray(opts.readPackageHook) ? opts.readPackageHook : [opts.readPackageHook]
  if (hooks.length === 0) return undefined
  const requestedByAlias = readRequestedSpecifiers(project.dependencySelectors)
  if (requestedByAlias.size === 0) return undefined
  const declared: ProjectManifest = mergeInstallSelectors(clone(project.originalManifest ?? project.manifest), {
    dependencySelectors: project.dependencySelectors,
    peer: project.peer,
    targetDependenciesField: project.targetDependenciesField,
  } as InstallSomeDepsMutation)
  const readDependencies = (manifest: ProjectManifest) => getAllDependenciesFromManifest(manifest, { autoInstallPeers: opts.autoInstallPeers, peerAliases: project.peerAliases })
  const declaredDependencies = readDependencies(declared)
  const overriddenDependencies = collector.applyOverrides == null
    ? undefined
    : readDependencies(await collector.applyOverrides(clone(declared), project.rootDir))
  const probed = await applyReadPackageHooks(hooks, { manifest: declared, rootDir: project.rootDir })
  return findRequestsTheHooksChange(requestedByAlias, {
    declared: declaredDependencies,
    overridden: overriddenDependencies,
    probed: readDependencies(probed),
  })
}

function readRequestedSpecifiers (dependencySelectors: string[]): Map<string, string | undefined> {
  const requestedByAlias = new Map<string, string | undefined>()
  for (const selector of dependencySelectors) {
    const { alias, bareSpecifier: requested } = parseWantedDependency(selector)
    if (alias == null) continue
    requestedByAlias.set(alias, requested)
  }
  return requestedByAlias
}

async function applyReadPackageHooks (
  hooks: ReadPackageHook[],
  { manifest, rootDir }: { manifest: ProjectManifest, rootDir: string }
): Promise<ProjectManifest> {
  let probed: ProjectManifest = manifest
  /* eslint-disable no-await-in-loop -- each hook receives the manifest returned by the previous one */
  for (const hook of hooks) {
    probed = await hook(probed, rootDir)
  }
  /* eslint-enable no-await-in-loop */
  return probed
}

function findRequestsTheHooksChange (
  requestedByAlias: Map<string, string | undefined>,
  dependencies: ProbedDependencies
): HookGovernedAdds | undefined {
  let superseded: Map<string, string> | undefined
  let removed: Set<string> | undefined
  for (const [alias, requested] of requestedByAlias) {
    if (requestSurvivesHooks(dependencies, { alias, requested })) continue
    const probedSpecifier = dependencies.probed[alias]
    if (probedSpecifier == null) {
      (removed ??= new Set()).add(alias)
      continue
    }
    (superseded ??= new Map()).set(alias, probedSpecifier)
  }
  return superseded == null && removed == null ? undefined : { superseded, removed }
}

function requestSurvivesHooks (
  { declared, overridden, probed }: ProbedDependencies,
  { alias, requested }: { alias: string, requested: string | undefined }
): boolean {
  const probedSpecifier = probed[alias]
  if (probedSpecifier === declared[alias]) return true
  return requested != null && overridden != null && probedSpecifier === overridden[alias]
}
