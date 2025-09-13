import type { PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/hooks.types'
import { PnpmError } from '@pnpm/error'
import { hookLogger } from '@pnpm/core-loggers'
import { createHashFromMultipleFiles } from '@pnpm/crypto.hash'
import pathAbsolute from 'path-absolute'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'
import { requirePnpmfile, type Pnpmfile, type Finders } from './requirePnpmfile.js'
import { type HookContext, type Hooks } from './Hooks.js'

// eslint-disable-next-line
type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

interface PnpmfileEntry {
  path: string
  includeInChecksum: boolean
  optional?: boolean
}

interface PnpmfileEntryLoaded {
  file: string
  hooks: Pnpmfile['hooks'] | undefined
  finders: Pnpmfile['finders'] | undefined
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

export interface RequireHooksResult {
  hooks: CookedHooks
  finders: Finders
  resolvedPnpmfilePaths: string[]
}

export async function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfiles?: string[]
    tryLoadDefaultPnpmfile?: boolean
  }
): Promise<RequireHooksResult> {
  const pnpmfiles: PnpmfileEntry[] = []
  if (opts.globalPnpmfile) {
    pnpmfiles.push({
      path: opts.globalPnpmfile,
      includeInChecksum: false,
    })
  }
  if (opts.tryLoadDefaultPnpmfile) {
    pnpmfiles.push({
      path: '.pnpmfile.cjs',
      includeInChecksum: true,
      optional: true,
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
  const entries: PnpmfileEntryLoaded[] = []
  const loadedFiles: string[] = []
  await Promise.all(pnpmfiles.map(async ({ path, includeInChecksum, optional }) => {
    const file = pathAbsolute(path, prefix)
    if (!loadedFiles.includes(file)) {
      loadedFiles.push(file)
      const requirePnpmfileResult = await requirePnpmfile(file, prefix)
      if (requirePnpmfileResult != null) {
        entries.push({
          file,
          includeInChecksum,
          hooks: requirePnpmfileResult.pnpmfileModule?.hooks,
          finders: requirePnpmfileResult.pnpmfileModule?.finders,
        })
      } else if (!optional) {
        throw new PnpmError('PNPMFILE_NOT_FOUND', `pnpmfile at "${file}" is not found`)
      }
    }
  }))

  const mergedFinders: Finders = {}
  const cookedHooks: CookedHooks & Required<Pick<CookedHooks, 'readPackage' | 'preResolution' | 'afterAllResolved' | 'filterLog' | 'updateConfig'>> = {
    readPackage: [],
    preResolution: [],
    afterAllResolved: [],
    filterLog: [],
    updateConfig: [],
  }

  // calculate combined checksum for all included files
  if (entries.some((entry) => entry.hooks != null)) {
    cookedHooks.calculatePnpmfileChecksum = async () => {
      const filesToIncludeInHash: string[] = []
      for (const { includeInChecksum, file } of entries) {
        if (includeInChecksum) {
          filesToIncludeInHash.push(file)
        }
      }
      filesToIncludeInHash.sort()
      return createHashFromMultipleFiles(filesToIncludeInHash)
    }
  }

  let importProvider: string | undefined
  let fetchersProvider: string | undefined
  const finderProviders: Record<string, string> = {}

  // process hooks in order
  for (const { hooks, file, finders } of entries) {
    if (finders != null) {
      for (const [finderName, finder] of Object.entries(finders)) {
        if (mergedFinders[finderName] != null) {
          const firstDefinedIn = finderProviders[finderName]
          throw new PnpmError(
            'DUPLICATE_FINDER',
            `Finder "${finderName}" defined in both ${firstDefinedIn} and ${file}`
          )
        }
        mergedFinders[finderName] = finder
        finderProviders[finderName] = file
      }
    }
    const fileHooks: Hooks = hooks ?? {}

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

  return {
    hooks: cookedHooks,
    finders: mergedFinders,
    resolvedPnpmfilePaths: entries.map(({ file }) => file),
  }
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
