import { streamParser } from '@pnpm/logger'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import { PackageUsages, StoreController } from '@pnpm/store-controller-types'
import { ReporterFunction } from './types'

export default async function (
  packageSelectors: string[],
  opts: {
    reporter?: ReporterFunction,
    storeController: StoreController,
  },
): Promise<{ [packageSelector: string]: PackageUsages[] }> {
  const reporter = opts?.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const packageSelectorsBySearchQueries = packageSelectors.reduce((acc, packageSelector) => {
    const searchQuery = parsedPackageSelectorToSearchQuery(parseWantedDependency(packageSelector))
    acc[searchQuery] = packageSelector
    return acc
  }, {})

  const packageUsagesBySearchQueries = await opts.storeController.findPackageUsages(Object.keys(packageSelectorsBySearchQueries))

  const results = {}

  for (const searchQuery of Object.keys(packageSelectorsBySearchQueries)) {
    results[packageSelectorsBySearchQueries[searchQuery]] = packageUsagesBySearchQueries[searchQuery] || []
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return results
}

function parsedPackageSelectorToSearchQuery (parsedPackageSelector: {alias: string} | {pref: string} | {alias: string, pref: string}) {
  if (!parsedPackageSelector['alias']) return parsedPackageSelector['pref']
  if (!parsedPackageSelector['pref']) return `/${parsedPackageSelector['alias']}/`
  return `/${parsedPackageSelector['alias']}/${parsedPackageSelector['pref']}`
}
