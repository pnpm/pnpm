import path from 'node:path'

import { getSlotShimNodePathOptions, linkBins, linkBinsOfPackages } from '@pnpm/bins.linker'
import { isPackagePlaced } from '@pnpm/building.during-install'
import type { DependenciesGraph, DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { DependencyManifest } from '@pnpm/types'
import { pickBy, props } from 'ramda'

import { limitLinking } from './limits.js'

interface LinkAllBinsOptions {
  depNodes?: DependenciesGraphNode[]
  enableGlobalVirtualStore?: boolean
  extraNodePaths?: string[]
  forceForPackages?: ReadonlySet<string>
  optional: boolean
  preferSymlinkedExecutables?: boolean
  warn: (message: string) => void
}

export async function linkAllBins (
  depGraph: DependenciesGraph,
  opts: LinkAllBinsOptions
): Promise<void> {
  const depNodes = opts.depNodes ?? Object.values(depGraph)
  await Promise.all(
    depNodes
      .map(async (depNode) => limitLinking(async () => linkBinsOfDepNode(depNode, depGraph, opts)))
  )
}

async function linkBinsOfDepNode (depNode: DependenciesGraphNode, depGraph: DependenciesGraph, opts: LinkAllBinsOptions): Promise<void> {
  // The build links the bins of a staged package in its staging directory.
  if (depNode.stagingDir != null) return
  const childrenToLink: Record<string, string> = opts.optional
    ? depNode.children
    : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)

  const binPath = path.join(depNode.dir, 'node_modules/.bin')
  const pkgSnapshots = props<string, DependenciesGraphNode>(Object.values(childrenToLink), depGraph)

  if (pkgSnapshots.includes(undefined as any)) { // eslint-disable-line
    await linkBins(depNode.modules, binPath, {
      ...getSlotShimNodePathOptions(opts),
      forceForPackages: opts.forceForPackages,
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
      ...getSlotShimNodePathOptions(opts),
      forceForPackages: opts.forceForPackages,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: opts.warn,
    })
  }
}

async function linkBinsOfChildren (children: DependenciesGraphNode[], binPath: string, opts: LinkAllBinsOptions): Promise<void> {
  const pkgs = await Promise.all(
    children
      .filter(({ hasBin }) => hasBin)
      .map(async ({ dir, stagingDir }) => ({
        location: dir,
        manifest: await readPackageJsonFromDir(stagingDir != null && !await isPackagePlaced(dir) ? stagingDir : dir) as DependencyManifest,
      }))
  )

  await linkBinsOfPackages(pkgs, binPath, {
    ...getSlotShimNodePathOptions(opts),
    forceForPackages: opts.forceForPackages,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })
}
