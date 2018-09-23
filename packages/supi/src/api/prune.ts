import logger, { streamParser } from '@pnpm/logger'
import { PackageJson } from '@pnpm/types'
import { removeOrphanPackages as removeOrphanPkgs } from '@pnpm/utils'
import {
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import extendOptions, {
  PruneOptions,
} from './extendPruneOptions'
import getContext from './getContext'
import { installPkgs } from './install'

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
    dependencies: opts.include.dependencies ? ctx.pkg.dependencies : {},
    devDependencies: opts.include.devDependencies ? ctx.pkg.devDependencies : {},
    optionalDependencies: opts.include.optionalDependencies ? ctx.pkg.optionalDependencies : {},
  } as PackageJson

  const warn = (message: string) => logger.warn({message, prefix: opts.prefix})
  const prunedShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg, ctx.importerPath, warn)

  await removeOrphanPkgs({
    bin: opts.bin,
    hoistedAliases: ctx.hoistedAliases,
    importerPath: ctx.importerPath,
    newShrinkwrap: prunedShr,
    oldShrinkwrap: ctx.currentShrinkwrap,
    prefix: ctx.prefix,
    pruneStore: true,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
  })

  if (opts.shamefullyFlatten) {
    await installPkgs(prunedShr.importers[ctx.importerPath].specifiers, {...opts, lock: false, reinstallForFlatten: true, update: false})
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
