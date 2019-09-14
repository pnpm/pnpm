import PnpmError from '@pnpm/error'
import {
  storeLogger,
  streamParser,
} from '@pnpm/logger'
import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/utils'
import parseWantedDependencies from './parseWantedDependencies'
import { ReporterFunction } from './types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    prefix?: string,
    registries?: Registries,
    reporter?: ReporterFunction,
    storeController: StoreController,
    tag?: string,
  },
) {
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

  let hasFailures = false
  const prefix = opts.prefix || process.cwd()
  const registries = opts.registries || {
    default: 'https://registry.npmjs.org/',
  }
  await Promise.all(deps.map(async (dep) => {
    try {
      const pkgResponse = await opts.storeController.requestPackage(dep, {
        downloadPriority: 1,
        lockfileDirectory: prefix,
        preferredVersions: {},
        prefix,
        registry: dep.alias && pickRegistryForPackage(registries, dep.alias) || registries.default,
      })
      await pkgResponse['files']() // tslint:disable-line:no-string-literal
      storeLogger.info(`+ ${pkgResponse.body.id}`)
    } catch (e) {
      hasFailures = true
      storeLogger.error(e)
    }
  }))

  await opts.storeController.saveState()

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  if (hasFailures) {
    throw new PnpmError('STORE_ADD_FAILURE', 'Some packages have not been added correctly')
  }
}
