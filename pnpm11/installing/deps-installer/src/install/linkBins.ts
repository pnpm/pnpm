import path from 'node:path'

import { getProjectNodePath, linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { linkBinsOfDependencies, linkBinsOfRuntimeDependencies } from '@pnpm/building.during-install'
import type { DependenciesGraph, DependenciesGraphNode } from '@pnpm/installing.deps-resolver'
import { logger } from '@pnpm/logger'
import type { DependencyManifest, DepPath } from '@pnpm/types'
import { safeReadPublishManifest } from '@pnpm/workspace.project-manifest-reader'
import pLimit from 'p-limit'
import { props } from 'ramda'

import type { LinkContext } from './linkResolvedProjects.js'
import type { ImporterToUpdate } from './mutationTypes.js'

const limitLinking = pLimit(16)

export async function linkRuntimeBinsOfImporters (opts: {
  dependenciesByProjectId: Record<string, Map<string, DepPath>>
  dependenciesGraph: DependenciesGraph
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
  projects: Array<Pick<ImporterToUpdate, 'binsDir' | 'id'>>
}
): Promise<void> {
  await Promise.all(opts.projects.map((project) => limitLinking(() =>
    linkBinsOfRuntimeDependencies(
      Array.from(opts.dependenciesByProjectId[project.id].values()).map((depPath) => opts.dependenciesGraph[depPath]),
      project.binsDir,
      {
        extraNodePaths: opts.extraNodePaths,
        preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      }
    )
  )))
}

/**
 * Links the bins of the newly added packages into their dependents, and the
 * bins of each project's direct dependencies into the project.
 */
export async function linkBinsOfResolvedProjects (step: LinkContext, newDepPaths: DepPath[] | undefined): Promise<void> {
  const { ctx, opts, projects, resolution } = step
  if (newDepPaths?.length && !opts.virtualStoreOnly) {
    const newPkgs = props<DepPath, DependenciesGraphNode>(
      newDepPaths.filter((depPath) => !ctx.skipped.has(depPath)),
      resolution.dependenciesGraph
    )
    await linkAllBins(newPkgs, resolution.dependenciesGraph, {
      extraNodePaths: ctx.extraNodePaths,
      optional: opts.include.optionalDependencies,
      warn: warnAboutBins.bind(null, opts.lockfileDir),
    })
  }

  if (!opts.virtualStoreOnly) await Promise.all(projects.map(async (project) => linkBinsOfProject(step, project)))
}

function warnAboutBins (prefix: string, message: string): void {
  logger.info({ message, prefix })
}

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
    optional: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  )
}

async function linkBinsOfProject (step: LinkContext, project: ImporterToUpdate): Promise<void> {
  const { ctx, opts } = step
  const projectModulesDir = await getProjectNodePath(project, { extendNodePath: opts.extendNodePath })
  const linkedPackages = ctx.publicHoistPattern?.length && path.relative(project.rootDir, opts.lockfileDir) === ''
    ? await linkBins(project.modulesDir, project.binsDir, {
      allowExoticManifests: true,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projectManifest: project.manifest,
      extraNodePaths: ctx.extraNodePaths,
      projectModulesDir,
      warn: warnAboutBins.bind(null, project.rootDir),
    })
    : await linkBinsOfDirectDependencies(step, { project, projectModulesDir })
  if (opts.global && project.mutation.includes('install')) {
    for (const pkg of project.wantedDependencies) {
      // This warning is never printed currently during "pnpm link --global"
      // due to the following issue: https://github.com/pnpm/pnpm/issues/4761
      if (pkg.alias && !linkedPackages?.includes(pkg.alias)) {
        logger.warn({ message: `${pkg.alias} has no binaries`, prefix: opts.lockfileDir })
      }
    }
  }
}

async function linkBinsOfDirectDependencies (
  { ctx, opts, resolution }: LinkContext,
  { project, projectModulesDir }: { project: ImporterToUpdate, projectModulesDir: string | undefined }
): Promise<string[]> {
  const directPkgs = [
    ...props<DepPath, DependenciesGraphNode>(
      Array.from(resolution.dependenciesByProjectId[project.id].values()).filter((depPath) => !ctx.skipped.has(depPath)),
      resolution.dependenciesGraph
    ),
    ...resolution.linkedDependenciesByProjectId[project.id].map(({ pkgId }) => ({
      dir: path.join(project.rootDir, pkgId.substring(5)),
      fetching: undefined,
    })),
  ]
  return linkBinsOfPackages(
    (
      await Promise.all(
        directPkgs.map(async (dep) => {
          const manifest = (await dep.fetching?.())?.bundledManifest ?? await safeReadPublishManifest(dep.dir)
          return {
            location: dep.dir,
            manifest,
          }
        })
      )
    )
      .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>,
    project.binsDir,
    {
      extraNodePaths: ctx.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projectModulesDir,
    }
  )
}
