import {streamParser} from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import {
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import extendOptions, {
  PruneOptions,
} from './extendPruneOptions'
import getContext from './getContext'
import {installPkgs} from './install'
import removeOrphanPkgs from './removeOrphanPkgs'

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
    bin: opts.bin,
    hoistedAliases: ctx.hoistedAliases,
    newShrinkwrap: prunedShr,
    oldShrinkwrap: ctx.currentShrinkwrap,
    prefix: ctx.root,
    pruneStore: true,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
  })

  if (opts.shamefullyFlatten) {
    await installPkgs(prunedShr.specifiers, {...opts, lock: false, reinstallForFlatten: true, update: false})
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
