import normalizeRegistryUrl = require('normalize-registry-url')
import {streamParser} from '@pnpm/logger'
import {StoreController} from 'package-store'
import {ReporterFunction} from '../types'
import parseWantedDependencies from '../parseWantedDependencies';

export default async function (
  fuzzyDeps: string[],
  opts: {
    prefix: string,
    registry?: string,
    verifyStoreIntegrity: boolean,
    reporter?: ReporterFunction,
    storeController: StoreController,
  },
) {
  const reporter = opts && opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const deps = await parseWantedDependencies(fuzzyDeps, {
    allowNew: true,
    currentPrefs: {},
    defaultTag: "",
    dev: false,
    devDependencies: {},
    optional: false,
    optionalDependencies: {},
  })

  const pkgIds = []
  for (let dep of deps) {
    const ret = await opts.storeController.requestPackage(dep, {
      downloadPriority: 1,
      prefix: opts.prefix,
      loggedPkg: {
        rawSpec: dep.raw,
      },
      registry: normalizeRegistryUrl(opts.registry || 'https://registry.npmjs.org/'),
      preferredVersions: {},
      verifyStoreIntegrity: opts.verifyStoreIntegrity,
    })
    pkgIds.push(ret.body.id)
  }

  await opts.storeController.updateConnections(opts.prefix, {
    addDependencies: pkgIds,
    removeDependencies: [],
    prune: false,
  })
  await opts.storeController.saveState()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
