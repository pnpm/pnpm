import path from 'node:path'

import type { LinkedDependency } from '@pnpm/installing.deps-resolver'
import type { LinkedDirectDep, ProjectToLink } from '@pnpm/installing.linking.direct-dep-linker'
import type { ProjectSnapshot } from '@pnpm/lockfile.fs'

import type { ImporterToUpdate } from './index.js'
import type { LinkContext, LinkPackagesOptions } from './link.js'

type DirectDepsLinkOptions = Pick<LinkPackagesOptions, 'dependenciesByProjectId' | 'linkedDependenciesByProjectId' | 'outdatedDependencies'>

export function getProjectsToLink (
  projects: ImporterToUpdate[],
  context: LinkContext,
  opts: DirectDepsLinkOptions
): Record<string, ProjectToLink> {
  return Object.fromEntries(projects.map((project) => {
    const importerFromLockfile = context.newCurrentLockfile.importers[project.id]
    return [project.id, {
      dir: project.rootDir,
      modulesDir: project.modulesDir,
      publishDir: project.manifest.publishConfig?.directory ?? importerFromLockfile?.publishDirectory,
      dependencies: [
        ...getLockedDirectDeps(project, { context, importerFromLockfile }, opts),
        ...opts.linkedDependenciesByProjectId[project.id].map((linkedDependency) => toExternalLinkedDirectDep(linkedDependency, project.rootDir)),
      ],
    }]
  }))
}

function getLockedDirectDeps (
  { id, manifest }: ImporterToUpdate,
  { context, importerFromLockfile }: { context: LinkContext, importerFromLockfile: ProjectSnapshot },
  opts: DirectDepsLinkOptions
): LinkedDirectDep[] {
  return Array.from(opts.dependenciesByProjectId[id].entries())
    .filter(([rootAlias]) => importerFromLockfile.specifiers[rootAlias])
    .map(([rootAlias, depPath]) => ({ rootAlias, depGraphNode: context.graph[depPath] }))
    .filter(({ depGraphNode }) => depGraphNode)
    .map(({ rootAlias, depGraphNode }) => ({
      alias: rootAlias,
      name: depGraphNode.name,
      version: depGraphNode.version,
      dir: depGraphNode.dir,
      id: depGraphNode.id,
      dependencyType: getDependencyType({
        dev: Boolean(manifest.devDependencies?.[depGraphNode.name]),
        optional: Boolean(manifest.optionalDependencies?.[depGraphNode.name]),
      }),
      latest: opts.outdatedDependencies[depGraphNode.id],
      isExternalLink: false,
    }))
}

function toExternalLinkedDirectDep (linkedDependency: LinkedDependency, rootDir: string): LinkedDirectDep {
  return {
    alias: linkedDependency.alias,
    name: linkedDependency.name,
    version: linkedDependency.version,
    dir: resolvePath(rootDir, linkedDependency.resolution.directory),
    id: linkedDependency.resolution.directory,
    dependencyType: getDependencyType({ dev: linkedDependency.dev, optional: linkedDependency.optional }),
    isExternalLink: true,
  }
}

function getDependencyType ({ dev, optional }: { dev?: boolean, optional?: boolean }): LinkedDirectDep['dependencyType'] {
  return (dev && 'dev' || optional && 'optional' || 'prod') as LinkedDirectDep['dependencyType']
}

const isAbsolutePath = /^\/|^[A-Z]:/i

// This function is copied from @pnpm/resolving.local-resolver
function resolvePath (where: string, spec: string): string {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}
