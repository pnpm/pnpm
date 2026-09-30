import path from 'node:path'

import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import type {
  DependencyManifest,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
} from '@pnpm/types'
import { isSubdir } from 'is-subdir'
import { difference, zipWith } from 'ramda'

import type { NodeId } from './nextNodeId.js'
import { createNodeIdForLinkedLocalPkg } from './resolveDependencies.js'
import type { LinkedDependency, ResolvedImporters } from './resolveDependencyTree.js'
import type { ResolveImporter } from './toResolveImporter.js'

export interface ProjectToLink {
  binsDir: string
  declaredDirectDependencies: Set<string>
  directNodeIdsByAlias: Map<string, NodeId>
  hoistedPeerProviderNodeIds: Set<NodeId>
  explicitlyRequestedDirectDependencies: Set<string>
  id: ProjectId
  linkedDependencies: LinkedDependency[]
  manifest: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
  topParents: Array<{ name: string, version: string }>
}

export interface ToProjectToLinkOptions {
  excludeLinksFromLockfile?: boolean
  lockfileDir: string
  resolvedImporter: ResolvedImporters[string]
}

export async function toProjectToLink (
  project: ResolveImporter,
  opts: ToProjectToLinkOptions
): Promise<ProjectToLink> {
  const { resolvedImporter } = opts
  const topParents = await getTopParentsOfProject(project, opts)
  return {
    binsDir: project.binsDir,
    declaredDirectDependencies: new Set([
      ...Object.keys(project.manifest == null ? {} : getAllDependenciesFromManifest(project.manifest)),
      ...project.wantedDependencies.flatMap(({ alias, isNew }) => isNew && alias != null ? [alias] : []),
    ]),
    directNodeIdsByAlias: resolvedImporter.directNodeIdsByAlias,
    hoistedPeerProviderNodeIds: resolvedImporter.hoistedPeerProviderNodeIds,
    explicitlyRequestedDirectDependencies: new Set(
      project.wantedDependencies.flatMap(({ alias, bareSpecifier, isNew, prevSpecifier, updateSpec }) =>
        alias != null && (isNew === true || updateSpec === true || (prevSpecifier != null && bareSpecifier !== prevSpecifier))
          ? [alias]
          : []
      )
    ),
    id: project.id,
    linkedDependencies: resolvedImporter.linkedDependencies,
    manifest: project.manifest,
    modulesDir: project.modulesDir,
    rootDir: project.rootDir,
    topParents,
  }
}

async function getTopParentsOfProject (
  project: ResolveImporter,
  opts: ToProjectToLinkOptions
): Promise<Array<{ name: string, version: string, alias?: string, linkedDir?: string }>> {
  const { resolvedImporter } = opts
  const topParents: Array<{ name: string, version: string, alias?: string, linkedDir?: string }> = project.manifest
    ? await getTopParents(
      difference(
        Object.keys(getAllDependenciesFromManifest(project.manifest)),
        resolvedImporter.directDependencies.map(({ alias }) => alias) || []
      ),
      project.modulesDir
    )
    : []
  for (const linkedDependency of resolvedImporter.linkedDependencies) {
    // The location of the external link may vary on different machines, so it is better not to include it in the lockfile.
    // As a workaround, we symlink to the root of node_modules, which is a symlink to the actual location of the external link.
    const target = !opts.excludeLinksFromLockfile || isSubdir(opts.lockfileDir, linkedDependency.resolution.directory)
      ? linkedDependency.resolution.directory
      : path.join(project.modulesDir, linkedDependency.alias)
    const linkedDir = createNodeIdForLinkedLocalPkg(opts.lockfileDir, target) as string
    topParents.push({
      name: linkedDependency.alias,
      version: linkedDependency.version,
      linkedDir,
    })
  }
  return topParents
}

async function getTopParents (pkgAliases: string[], modulesDir: string): Promise<DependencyManifest[]> {
  const pkgs = await Promise.all(
    pkgAliases.map((alias) => path.join(modulesDir, alias)).map(safeReadPackageJsonFromDir)
  )
  return zipWith((manifest, alias) => {
    if (!manifest) return null
    return {
      alias,
      name: manifest.name,
      version: manifest.version,
    }
  }, pkgs, pkgAliases)
    .filter(Boolean) as DependencyManifest[]
}
