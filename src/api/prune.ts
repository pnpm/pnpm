import {PackageJson} from '@pnpm/types'
import getContext from './getContext'
import extendOptions, {
  PruneOptions,
} from './extendPruneOptions'
import removeOrphanPkgs from './removeOrphanPkgs'
import {
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import {streamParser} from '@pnpm/logger'
import {installPkgs} from './install'

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
    hoistedAliases: ctx.hoistedAliases,
  })

  if (opts.shamefullyFlatten) {
    await installPkgs(prunedShr.specifiers, {...opts, lock: false, reinstallForFlatten: true, update: false})
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
