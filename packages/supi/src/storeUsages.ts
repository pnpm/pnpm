import { storeLogger, streamParser } from '@pnpm/logger'
import { FindPackageUsagesResponse, StoreController } from '@pnpm/store-controller-types'
import parseWantedDependencies from './parseWantedDependencies';
import { ReporterFunction } from './types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    reporter?: ReporterFunction,
    storeController: StoreController,
    tag?: string
  },
): Promise<FindPackageUsagesResponse[]> {
  const reporter = opts && opts.reporter;
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
  });

  const packageUsages: FindPackageUsagesResponse[] = await opts.storeController.findPackageUsages(deps);

  if (!packageUsages) {
    storeLogger.error(new Error('Internal error retrieving package usages'));
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return packageUsages as FindPackageUsagesResponse[];
}
