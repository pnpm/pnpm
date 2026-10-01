import path from 'node:path'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { isRuntimeDepPath } from '@pnpm/deps.path'
import { logger } from '@pnpm/logger'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'
import { pickBy } from 'ramda'

import type { DependenciesGraph, DependenciesGraphNode } from './buildGraph.js'

export async function linkBinsOfDependencies<NodeId extends string> (
  depNode: DependenciesGraphNode<NodeId>,
  depGraph: DependenciesGraph<NodeId>,
  opts: {
    extraNodePaths?: string[]
    optional: boolean
    preferSymlinkedExecutables?: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  const childrenToLink: Record<string, NodeId> = opts.optional
    ? depNode.children
    : pickBy((child, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)

  const binPath = path.join(depNode.dir, 'node_modules/.bin')

  const pkgNodes = [
    ...Object.entries(childrenToLink)
      .map(([alias, childDepPath]) => ({ alias, dep: depGraph[childDepPath] }))
      .filter(({ alias, dep }) => {
        if (!dep) {
          // TODO: Try to reproduce this issue with a test in @pnpm/installing.deps-installer
          logger.debug({ message: `Failed to link bins of "${alias}" to "${binPath}". This is probably not an issue.` })
          return false
        }
        return dep.hasBin && dep.installable !== false
      })
      .map(({ dep }) => dep),
    depNode,
  ]
  const pkgs = await Promise.all(pkgNodes
    .map(async (dep) => ({
      location: dep.dir,
      manifest: ((await dep.fetching?.())?.bundledManifest ?? (await safeReadPackageJsonFromDir(dep.dir))) as DependencyManifest ?? {},
    }))
  )

  await linkBinsOfPackages(pkgs, binPath, {
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })

  // link also the bundled dependencies` bins
  if (depNode.hasBundledDependencies) {
    const bundledModules = path.join(depNode.dir, 'node_modules')
    await linkBins(bundledModules, binPath, {
      extraNodePaths: opts.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: opts.warn,
    })
  }
}

export async function linkBinsOfRuntimeDependencies<NodeId extends string> (
  depNodes: Array<DependenciesGraphNode<NodeId> | undefined>,
  binPath: string,
  opts: {
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
  }
): Promise<void> {
  const runtimeNodes = depNodes.filter((dep): dep is DependenciesGraphNode<NodeId> => dep != null && isRuntimeDepPath(dep.depPath))
  if (runtimeNodes.length === 0) return
  const pkgs = await Promise.all(runtimeNodes.map(async (dep) => ({
    location: dep.dir,
    manifest: ((await dep.fetching?.())?.bundledManifest ?? (await safeReadPackageJsonFromDir(dep.dir))) as DependencyManifest ?? {},
  })))
  await linkBinsOfPackages(pkgs, binPath, opts)
}
