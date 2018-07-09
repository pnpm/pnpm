import normalizeRegistryUrl = require('normalize-registry-url')

import {streamParser} from '@pnpm/logger'
import {StoreController} from 'package-store'

import parseWantedDependencies from '../parseWantedDependencies';
import {ReporterFunction} from '../types'

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
    defaultTag: '',
    dev: false,
    devDependencies: {},
    optional: false,
    optionalDependencies: {},
  })

  const pkgIds = []
  for (const dep of deps) {
    const ret = await opts.storeController.requestPackage(dep, {
      downloadPriority: 1,
      loggedPkg: {
        rawSpec: dep.raw,
      },
      preferredVersions: {},
      prefix: opts.prefix,
      registry: normalizeRegistryUrl(opts.registry || 'https://registry.npmjs.org/'),
      verifyStoreIntegrity: opts.verifyStoreIntegrity,
    })
    pkgIds.push(ret.body.id)
  }

  await opts.storeController.updateConnections(opts.prefix, {
    addDependencies: pkgIds,
    prune: false,
    removeDependencies: [],
  })
  await opts.storeController.saveState()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
