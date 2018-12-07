import { storeLogger, streamParser } from '@pnpm/logger'
import { PackageUsage, StoreController } from '@pnpm/store-controller-types'
import parseWantedDependencies from './parseWantedDependencies'
import { ReporterFunction } from './types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    reporter?: ReporterFunction,
    storeController: StoreController,
    tag?: string
  },
): Promise<PackageUsage[]> {
  const reporter = opts && opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const deps = parseWantedDependencies(fuzzyDeps, {
    allowNew: true,
    currentPrefs: {},
    defaultTag: opts.tag || 'latest',
    dev: false,
    devDependencies: {},
    optional: false,
    optionalDependencies: {},
  })

  const packageUsages: PackageUsage[] = await opts.storeController.findPackageUsages(deps)

  if (!packageUsages) {
    storeLogger.error(new Error('Internal error retrieving package usages'))
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return packageUsages as PackageUsage[]
}
