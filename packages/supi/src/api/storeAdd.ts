import normalizeRegistryUrl = require('normalize-registry-url')

import logger, {streamParser} from '@pnpm/logger'
import {StoreController} from 'package-store'

import parseWantedDependencies from '../parseWantedDependencies';
import {ReporterFunction} from '../types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    prefix?: string,
    registry?: string,
    reporter?: ReporterFunction,
    storeController: StoreController,
    tag?: string,
    verifyStoreIntegrity?: boolean,
  },
) {
  const reporter = opts && opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const deps = await parseWantedDependencies(fuzzyDeps, {
    allowNew: true,
    currentPrefs: {},
    defaultTag: opts.tag || 'latest',
    dev: false,
    devDependencies: {},
    optional: false,
    optionalDependencies: {},
  })

  let hasFailures = false;
  await Promise.all(deps.map(async (dep) => {
    try {
      const pkgResponse = await opts.storeController.requestPackage(dep, {
        downloadPriority: 1,
        loggedPkg: {
          rawSpec: dep.raw,
        },
        preferredVersions: {},
        prefix: opts.prefix || process.cwd(),
        registry: normalizeRegistryUrl(opts.registry || 'https://registry.npmjs.org/'),
        verifyStoreIntegrity: opts.verifyStoreIntegrity || true,
      })
      await pkgResponse['fetchingFiles'] // tslint:disable-line:no-string-literal
      logger.info(`+ ${pkgResponse.body.id}`)
    } catch (e) {
      hasFailures = true;
      logger.error(e);
    }
  }))

  await opts.storeController.saveState()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  if (hasFailures) {
    const err = new Error('Some packages have not been added correctly')
    err['code'] = 'ERR_PNPM_STORE_ADD_FAILURE' // tslint:disable-line:no-string-literal
    throw err
  }
}
