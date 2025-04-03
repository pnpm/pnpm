import { PnpmError } from '@pnpm/error'
import { logger, globalInfo, streamParser } from '@pnpm/logger'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { type StoreController } from '@pnpm/store-controller-types'
import { type SupportedArchitectures } from '@pnpm/types'
import { type ReporterFunction } from './types'

export async function storeAdd (
  fuzzyDeps: string[],
  opts: {
    prefix?: string
    reporter?: ReporterFunction
    storeController: StoreController
    tag?: string
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<void> {
  const reporter = opts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const deps = fuzzyDeps.map((dep) => parseWantedDependency(dep))

  let hasFailures = false
  const prefix = opts.prefix ?? process.cwd()
  await Promise.all(deps.map(async (dep) => {
    try {
      const pkgResponse = await opts.storeController.requestPackage(dep, {
        downloadPriority: 1,
        lockfileDir: prefix,
        preferredVersions: {},
        projectDir: prefix,
        supportedArchitectures: opts.supportedArchitectures,
      })
      await pkgResponse.fetching!()
      globalInfo(`+ ${pkgResponse.body.id}`)
    } catch (e: any) { // eslint-disable-line
      hasFailures = true
      logger('store').error(e)
    }
  }))

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  if (hasFailures) {
    throw new PnpmError('STORE_ADD_FAILURE', 'Some packages have not been added correctly')
  }
}
