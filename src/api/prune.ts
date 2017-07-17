import path = require('path')
import R = require('ramda')
import getContext from './getContext'
import {PnpmOptions, Package} from '../types'
import extendOptions from './extendOptions'
import getPkgDirs from '../fs/getPkgDirs'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import lock from './lock'
import removeOrphanPkgs from './removeOrphanPkgs'
import {PackageSpec} from 'package-store'
import {
  ResolvedDependencies,
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import streamParser from '../logging/streamParser'

export async function prune(maybeOpts?: PnpmOptions): Promise<void> {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (opts.lock === false) {
    await run()
  } else {
    await lock(ctx.storePath, run, {stale: opts.lockStaleDuration, locks: opts.locks})
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function run () {
    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot prune')
    }

    const pkg = !opts.production ? ctx.pkg : {
      dependencies: ctx.pkg.dependencies,
      optionalDependencies: ctx.pkg.optionalDependencies,
    } as Package

    const prunedShr = pruneShrinkwrap(ctx.shrinkwrap, pkg)

    await removeOrphanPkgs({
      oldShrinkwrap: ctx.privateShrinkwrap,
      newShrinkwrap: prunedShr,
      prefix: ctx.root,
      store: ctx.storePath,
      storeIndex: ctx.storeIndex,
    })
  }
}
