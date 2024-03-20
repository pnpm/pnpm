import zipObj from 'ramda/src/zipObj'
import partition from 'ramda/src/partition'

import type {
  PkgAddress,
  PendingNode,
  DependenciesTree,
  LinkedDependency,
  ParentPkgAliases,
  WantedDependency,
  ResolvedImporters,
  ImporterToResolveDeps,
  DependenciesGraphNode,
  ChildrenByParentDepPath,
  ImporterToResolveGeneric,
  ResolvedPackagesByDepPath,
  ResolveDependenciesOptions,
} from '@pnpm/types'

import {
  resolveRootDependencies,
} from './resolveDependencies'
import { createNodeId, nodeIdContainsSequence } from './nodeIdUtils'

export * from './nodeIdUtils'

export async function resolveDependencyTree<T>(
  importers: Array<ImporterToResolveGeneric<T>>,
  opts: ResolveDependenciesOptions
) {
  const wantedToBeSkippedPackageIds = new Set<string>()

  const ctx = {
    autoInstallPeers: opts.autoInstallPeers === true,
    autoInstallPeersFromHighestMatch:
      opts.autoInstallPeersFromHighestMatch === true,
    allowBuild: opts.allowBuild,
    allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
    childrenByParentDepPath: {} as ChildrenByParentDepPath,
    currentLockfile: opts.currentLockfile,
    defaultTag: opts.tag,
    dependenciesTree: new Map() as DependenciesTree<DependenciesGraphNode>,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    forceFullResolution: opts.forceFullResolution,
    ignoreScripts: opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? -1,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {} as { [pkgId: string]: string },
    patchedDependencies: opts.patchedDependencies,
    pendingNodes: [] as PendingNode[],
    pnpmVersion: opts.pnpmVersion,
    preferWorkspacePackages: opts.preferWorkspacePackages,
    readPackageHook: opts.hooks.readPackage,
    registries: opts.registries,
    resolvedPackagesByDepPath: {} as ResolvedPackagesByDepPath,
    resolutionMode: opts.resolutionMode,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
    appliedPatches: new Set<string>(),
    updatedSet: new Set<string>(),
    workspacePackages: opts.workspacePackages,
    missingPeersOfChildrenByPkgId: {},
  }

  const resolveArgs: ImporterToResolveDeps[] = importers.map((importer: ImporterToResolveGeneric<T>): ImporterToResolveDeps => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]

    // This may be optimized.
    // We only need to proceed resolving every dependency
    // if the newly added dependency has peer dependencies.
    const proceed =
      importer.id === '.' ||
      importer.hasRemovedDependencies === true ||
      importer.wantedDependencies.some((wantedDep: any) => wantedDep.isNew) // eslint-disable-line @typescript-eslint/no-explicit-any

    const resolveOpts = {
      currentDepth: 0,
      parentPkg: {
        installable: true,
        nodeId: `>${importer.id}>`,
        optional: false,
        depPath: importer.id,
        rootDir: importer.rootDir,
      },
      proceed,
      resolvedDependencies: {
        ...projectSnapshot.dependencies,
        ...projectSnapshot.devDependencies,
        ...projectSnapshot.optionalDependencies,
      },
      updateDepth: -1,
      updateMatching: importer.updateMatching,
      prefix: importer.rootDir,
      supportedArchitectures: opts.supportedArchitectures,
      updateToLatest: opts.updateToLatest,
    }

    return {
      updatePackageManifest: importer.updatePackageManifest,
      parentPkgAliases: Object.fromEntries(
        importer.wantedDependencies
          .filter(({ alias }) => alias)
          .map(({ alias }) => [alias, true])
      ) as ParentPkgAliases,
      preferredVersions: importer.preferredVersions ?? {},
      wantedDependencies: importer.wantedDependencies,
      options: resolveOpts,
    }
  })

  const { pkgAddressesByImporters, time } = await resolveRootDependencies(
    ctx,
    resolveArgs
  )

  const directDepsByImporterId = zipObj(
    importers.map(({ id }) => id),
    pkgAddressesByImporters
  )

  ctx.pendingNodes.forEach((pendingNode) => {
    ctx.dependenciesTree.set(pendingNode.nodeId, {
      children: () =>
        buildTree(
          ctx,
          pendingNode.nodeId,
          pendingNode.resolvedPackage.id ?? '',
          ctx.childrenByParentDepPath[pendingNode.resolvedPackage.depPath],
          pendingNode.depth + 1,
          pendingNode.installable
        ),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    })
  })

  const resolvedImporters: ResolvedImporters = {}

  for (const { id, wantedDependencies } of importers) {
    const directDeps = dedupeSameAliasDirectDeps(
      directDepsByImporterId[id],
      wantedDependencies
    )

    const [linkedDependencies, directNonLinkedDeps] = partition(
      (dep: PkgAddress | LinkedDependency): boolean => {
        return dep.isLinkedDependency === true;
      },
      directDeps
    ) as [LinkedDependency[], PkgAddress[]]

    resolvedImporters[id] = {
      directDependencies: directDeps.map((dep: PkgAddress | LinkedDependency) => {
        if (dep.isLinkedDependency === true) {
          return dep
        }

        const resolvedPackage = ctx.dependenciesTree.get(dep.nodeId ?? '')?.resolvedPackage

        return {
          alias: dep.alias,
          // @ts-ignore
          dev: resolvedPackage?.dev,
          name: resolvedPackage?.name,
          normalizedPref: dep.normalizedPref,
          // @ts-ignore
          optional: resolvedPackage?.optional,
          // @ts-ignore
          pkgId: resolvedPackage?.id,
          // @ts-ignore
          resolution: resolvedPackage?.resolution,
          version: resolvedPackage?.version,
        }
      }),
      directNodeIdsByAlias: directNonLinkedDeps.reduce(
        (acc: Record<string, string>, { alias, nodeId }): Record<string, string> => {
          // @ts-ignore
          acc[alias] = nodeId

          return acc
        },
        {}
      ),
      linkedDependencies,
    }
  }

  return {
    dependenciesTree: ctx.dependenciesTree,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByDepPath: ctx.resolvedPackagesByDepPath,
    wantedToBeSkippedPackageIds,
    appliedPatches: ctx.appliedPatches,
    time,
  }
}

function buildTree(
  ctx: {
    childrenByParentDepPath: ChildrenByParentDepPath
    dependenciesTree: DependenciesTree<DependenciesGraphNode>
    resolvedPackagesByDepPath: ResolvedPackagesByDepPath
    skipped: Set<string>
  },
  parentNodeId: string,
  parentId: string,
  children: Array<{ alias: string; depPath: string }>,
  depth: number,
  installable: boolean
) {
  const childrenNodeIds: Record<string, string> = {}

  for (const child of children) {
    if (child.depPath.startsWith('link:')) {
      childrenNodeIds[child.alias] = child.depPath
      continue
    }

    if (
      nodeIdContainsSequence(parentNodeId, parentId, child.depPath) ||
      parentId === child.depPath
    ) {
      continue
    }

    const childNodeId = createNodeId(parentNodeId, child.depPath)

    childrenNodeIds[child.alias] = childNodeId

    installable = installable && !ctx.skipped.has(child.depPath)

    ctx.dependenciesTree.set(childNodeId, {
      children: () =>
        buildTree(
          ctx,
          childNodeId,
          child.depPath,
          ctx.childrenByParentDepPath[child.depPath],
          depth + 1,
          installable
        ),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPackagesByDepPath[child.depPath],
    })
  }

  return childrenNodeIds
}

/**
 * There may be cases where multiple dependencies have the same alias in the directDeps array.
 * E.g., when there is "is-negative: github:kevva/is-negative#1.0.0" in the package.json dependencies,
 * and then re-execute `pnpm add github:kevva/is-negative#1.0.1`.
 * In order to make sure that the latest 1.0.1 version is installed, we need to remove the duplicate dependency.
 * fix https://github.com/pnpm/pnpm/issues/6966
 */
function dedupeSameAliasDirectDeps(
  directDeps: Array<PkgAddress | LinkedDependency>,
  wantedDependencies: Array<WantedDependency & { isNew?: boolean | undefined }>
): (PkgAddress | LinkedDependency)[] {
  const deps = new Map<string, PkgAddress | LinkedDependency>()

  for (const directDep of directDeps) {
    const { alias, normalizedPref } = directDep
    if (!deps.has(alias)) {
      deps.set(alias, directDep)
    } else {
      const wantedDep = wantedDependencies.find((dep) =>
        dep.alias ? dep.alias === alias : dep.pref === normalizedPref
      )
      if (wantedDep?.isNew) {
        deps.set(alias, directDep)
      }
    }
  }
  return Array.from(deps.values())
}
