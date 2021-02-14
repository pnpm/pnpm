import PnpmError from '@pnpm/error'
import logger, { globalInfo, streamParser } from '@pnpm/logger'
import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import { StoreController } from '@pnpm/store-controller-types'
import { Registries } from '@pnpm/types'
import { ReporterFunction } from './types'

export default async function (
  fuzzyDeps: string[],
  opts: {
    prefix?: string
    registries?: Registries
    reporter?: ReporterFunction
    storeController: StoreController
    tag?: string
  }
) {
  const reporter = opts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const deps = fuzzyDeps.map((dep) => parseWantedDependency(dep))

  let hasFailures = false
  const prefix = opts.prefix ?? process.cwd()
  const registries = opts.registries ?? {
    default: 'https://registry.npmjs.org/',
  }
  await Promise.all(deps.map(async (dep) => {
    try {
      const pkgResponse = await opts.storeController.requestPackage(dep, {
        downloadPriority: 1,
        lockfileDir: prefix,
        preferredVersions: {},
        projectDir: prefix,
        registry: (dep.alias && pickRegistryForPackage(registries, dep.alias)) ?? registries.default,
      })
      await pkgResponse.files!()
      globalInfo(`+ ${pkgResponse.body.id}`)
    } catch (e) {
      hasFailures = true
      logger('store').error(e)
    }
  }))

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  if (hasFailures) {
    throw new PnpmError('STORE_ADD_FAILURE', 'Some packages have not been added correctly')
  }
}
