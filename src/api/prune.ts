import {PackageJson} from '@pnpm/types'
import path = require('path')
import R = require('ramda')
import getContext from './getContext'
import {PnpmOptions} from '@pnpm/types'
import extendOptions, {
  PruneOptions,
  StrictPruneOptions,
} from './extendPruneOptions'
import getPkgDirs from '../fs/getPkgDirs'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import removeOrphanPkgs from './removeOrphanPkgs'
import {
  ResolvedDependencies,
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import {streamParser} from '@pnpm/logger'

export async function prune (
  maybeOpts: PruneOptions,
): Promise<void> {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (!ctx.pkg) {
    throw new Error('No package.json found - cannot prune')
  }

  const pkg = {
    dependencies: opts.production ? ctx.pkg.dependencies : {},
    devDependencies: opts.development ? ctx.pkg.devDependencies : {},
    optionalDependencies: opts.optional ? ctx.pkg.optionalDependencies : {},
  } as PackageJson

  const prunedShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg)

  await removeOrphanPkgs({
    oldShrinkwrap: ctx.currentShrinkwrap,
    newShrinkwrap: prunedShr,
    prefix: ctx.root,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
    pruneStore: true,
    bin: opts.bin,
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
