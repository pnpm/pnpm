import path from 'node:path'

import { linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import type { DependenciesGraph, DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'
import { pickBy, props } from 'ramda'

import { limitLinking } from './limits.js'

interface LinkAllBinsOptions {
  extraNodePaths?: string[]
  optional: boolean
  preferSymlinkedExecutables?: boolean
  warn: (message: string) => void
}

export async function linkAllBins (
  depGraph: DependenciesGraph,
  opts: LinkAllBinsOptions
): Promise<void> {
  await Promise.all(
    Object.values(depGraph)
      .map(async (depNode) => limitLinking(async () => linkBinsOfDepNode(depNode, depGraph, opts)))
  )
}

async function linkBinsOfDepNode (depNode: DependenciesGraphNode, depGraph: DependenciesGraph, opts: LinkAllBinsOptions): Promise<void> {
  const childrenToLink: Record<string, string> = opts.optional
    ? depNode.children
    : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)

  const binPath = path.join(depNode.dir, 'node_modules/.bin')
  const pkgSnapshots = props<string, DependenciesGraphNode>(Object.values(childrenToLink), depGraph)

  if (pkgSnapshots.includes(undefined as any)) { // eslint-disable-line
    await linkBins(depNode.modules, binPath, {
      extraNodePaths: opts.extraNodePaths,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: opts.warn,
    })
  } else {
    await linkBinsOfChildren(pkgSnapshots, binPath, opts)
  }

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

async function linkBinsOfChildren (children: DependenciesGraphNode[], binPath: string, opts: LinkAllBinsOptions): Promise<void> {
  const pkgs = await Promise.all(
    children
      .filter(({ hasBin }) => hasBin)
      .map(async ({ dir }) => ({
        location: dir,
        manifest: await readPackageJsonFromDir(dir) as DependencyManifest,
      }))
  )

  await linkBinsOfPackages(pkgs, binPath, {
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })
}
