import normalizeRegistryUrl = require('normalize-registry-url')

import {streamParser} from '@pnpm/logger'
import {StoreController} from 'package-store'

import parseWantedDependencies from '../parseWantedDependencies';
import {ReporterFunction} from '../types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    registry?: string,
    verifyStoreIntegrity?: boolean,
    reporter?: ReporterFunction,
    storeController: StoreController,
    tag?: string,
  },
) {
  const reporter = opts && opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const deps = await parseWantedDependencies(fuzzyDeps, {
    allowNew: true,
    currentPrefs: {},
    defaultTag: opts.tag || 'default',
    dev: false,
    devDependencies: {},
    optional: false,
    optionalDependencies: {},
  })

  await Promise.all(deps.map(async (dep) => {
    const pkgResponse = await opts.storeController.requestPackage(dep, {
      downloadPriority: 1,
      loggedPkg: {
        rawSpec: dep.raw,
      },
      preferredVersions: {},
      prefix: '',
      registry: normalizeRegistryUrl(opts.registry || 'https://registry.npmjs.org/'),
      verifyStoreIntegrity: opts.verifyStoreIntegrity || true,
    })
    await pkgResponse['fetchingFiles'].catch((err: Error) => {}) // tslint:disable-line
  }))

  await opts.storeController.saveState()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}
