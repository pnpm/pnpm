import type { PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/hooks.types'
import { PnpmError } from '@pnpm/error'
import { hookLogger } from '@pnpm/core-loggers'
import { createHashFromFile } from '@pnpm/crypto.hash'
import { createHash } from 'crypto'
import pathAbsolute from 'path-absolute'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'
import { requirePnpmfile } from './requirePnpmfile'
import { type HookContext, type Hooks } from './Hooks'

// eslint-disable-next-line
type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

export interface PnpmfileEntry {
  path: string
  includeInChecksum: boolean
}

export interface CookedHooks {
  readPackage?: Array<Cook<Required<Hooks>['readPackage']>>
  preResolution?: Array<Cook<Required<Hooks>['preResolution']>>
  afterAllResolved?: Array<Cook<Required<Hooks>['afterAllResolved']>>
  filterLog?: Array<Cook<Required<Hooks>['filterLog']>>
  updateConfig?: Array<Cook<Required<Hooks>['updateConfig']>>
  importPackage?: ImportIndexedPackageAsync
  fetchers?: CustomFetchers
  calculatePnpmfileChecksum?: () => Promise<string>
}

export function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfiles?: string[]
  }
): CookedHooks {
  const pnpmfiles: PnpmfileEntry[] = []
  if (opts.globalPnpmfile) {
    pnpmfiles.push({
      path: opts.globalPnpmfile,
      includeInChecksum: false,
    })
  }
  if (opts.pnpmfiles) {
    for (const pnpmfile of opts.pnpmfiles) {
      pnpmfiles.push({
        path: pnpmfile,
        includeInChecksum: true,
      })
    }
  }
  const entries = pnpmfiles.map(({ path, includeInChecksum }) => ({
    file: pathAbsolute(path, prefix),
    includeInChecksum,
    module: requirePnpmfile(pathAbsolute(path, prefix), prefix),
  })) ?? []

  const cookedHooks: CookedHooks & Required<Pick<CookedHooks, 'readPackage' | 'preResolution' | 'afterAllResolved' | 'filterLog' | 'updateConfig'>> = {
    readPackage: [],
    preResolution: [],
    afterAllResolved: [],
    filterLog: [],
    updateConfig: [],
  }

  // calculate combined checksum for all included files
  if (entries.length > 0) {
    cookedHooks.calculatePnpmfileChecksum = async () => {
      const checksums = await Promise.all(
        entries
          .filter((e) => e.includeInChecksum)
          .map((e) => createHashFromFile(e.file))
      )
      const hasher = createHash('sha256')
      for (const sum of checksums) {
        hasher.update(sum)
      }
      return hasher.digest('hex')
    }
  }

  let importProvider: string | undefined
  let fetchersProvider: string | undefined

  // process hooks in order
  for (const { module, file } of entries) {
    const fileHooks: Hooks = module?.hooks ?? {}

    // readPackage & afterAllResolved
    for (const hookName of ['readPackage', 'afterAllResolved'] as const) {
      const fn = fileHooks[hookName]
      if (fn) {
        const context = createReadPackageHookContext(file, prefix, hookName)
        cookedHooks[hookName].push((pkg: object) => fn(pkg as any, context)) // eslint-disable-line @typescript-eslint/no-explicit-any
      }
    }

    // filterLog
    if (fileHooks.filterLog) {
      cookedHooks.filterLog.push(fileHooks.filterLog)
    }

    // updateConfig
    if (fileHooks.updateConfig) {
      const updateConfig = fileHooks.updateConfig
      cookedHooks.updateConfig.push((config: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
        const updated = updateConfig(config)
        if (updated == null) {
          throw new PnpmError('CONFIG_IS_UNDEFINED', 'The updateConfig hook returned undefined')
        }
        return updated
      })
    }

    // preResolution
    if (fileHooks.preResolution) {
      const preRes = fileHooks.preResolution
      cookedHooks.preResolution.push((ctx: PreResolutionHookContext) => preRes(ctx, createPreResolutionHookLogger(prefix)))
    }

    // importPackage: only one allowed
    if (fileHooks.importPackage) {
      if (importProvider) {
        throw new PnpmError(
          'MULTIPLE_IMPORT_PACKAGE',
          `importPackage hook defined in both ${importProvider} and ${file}`
        )
      }
      importProvider = file
      cookedHooks.importPackage = fileHooks.importPackage
    }

    // fetchers: only one allowed
    if (fileHooks.fetchers) {
      if (fetchersProvider) {
        throw new PnpmError(
          'MULTIPLE_FETCHERS',
          `fetchers hook defined in both ${fetchersProvider} and ${file}`
        )
      }
      fetchersProvider = file
      cookedHooks.fetchers = fileHooks.fetchers
    }
  }

  return cookedHooks
}

function createReadPackageHookContext (calledFrom: string, prefix: string, hook: string): HookContext {
  return {
    log: (message: string) => {
      hookLogger.debug({ from: calledFrom, hook, message, prefix })
    },
  }
}

function createPreResolutionHookLogger (prefix: string): PreResolutionHookLogger {
  const hook = 'preResolution'
  return {
    info: (message: string) => {
      hookLogger.info({ message, prefix, hook } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    },
    warn: (message: string) => {
      hookLogger.warn({ message, prefix, hook } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    },
  }
}
