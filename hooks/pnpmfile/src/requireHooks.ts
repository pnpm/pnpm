import type { PreResolutionHookContext, PreResolutionHookLogger, CustomResolver, CustomFetcher } from '@pnpm/hooks.types'
import { PnpmError } from '@pnpm/error'
import { hookLogger } from '@pnpm/core-loggers'
import { createHashFromMultipleFiles } from '@pnpm/crypto.hash'
import pathAbsolute from 'path-absolute'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'
import { type ReadPackageHook, type BeforePackingHook, type BaseManifest } from '@pnpm/types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { requirePnpmfile, type Pnpmfile, type Finders } from './requirePnpmfile.js'
import { type Hooks, type HookContext } from './Hooks.js'

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
  resolvers: Pnpmfile['resolvers'] | undefined
  fetchers: Pnpmfile['fetchers'] | undefined
  includeInChecksum: boolean
}

export interface CookedHooks {
  readPackage?: ReadPackageHook[]
  beforePacking?: BeforePackingHook[]
  preResolution?: Array<(ctx: PreResolutionHookContext) => Promise<void>>
  afterAllResolved?: Array<(lockfile: LockfileObject) => LockfileObject | Promise<LockfileObject>>
  filterLog?: Array<Cook<Required<Hooks>['filterLog']>>
  updateConfig?: Array<Cook<Required<Hooks>['updateConfig']>>
  importPackage?: ImportIndexedPackageAsync
  customResolvers?: CustomResolver[]
  customFetchers?: CustomFetcher[]
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
  const entries: PnpmfileEntryLoaded[] = []
  const loadedFiles: string[] = []
  if (opts.tryLoadDefaultPnpmfile) {
    // Prefer .pnpmfile.mjs over .pnpmfile.cjs. Only load one.
    const mjsPath = pathAbsolute('.pnpmfile.mjs', prefix)
    const mjsResult = await requirePnpmfile(mjsPath, prefix)
    if (mjsResult != null) {
      loadedFiles.push(mjsPath)
      entries.push({
        file: mjsPath,
        includeInChecksum: true,
        hooks: mjsResult.pnpmfileModule?.hooks,
        finders: mjsResult.pnpmfileModule?.finders,
        resolvers: mjsResult.pnpmfileModule?.resolvers,
        fetchers: mjsResult.pnpmfileModule?.fetchers,
      })
    } else {
      pnpmfiles.push({
        path: '.pnpmfile.cjs',
        includeInChecksum: true,
        optional: true,
      })
    }
  }
  if (opts.pnpmfiles) {
    for (const pnpmfile of opts.pnpmfiles) {
      pnpmfiles.push({
        path: pnpmfile,
        includeInChecksum: true,
      })
    }
  }
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
          resolvers: requirePnpmfileResult.pnpmfileModule?.resolvers,
          fetchers: requirePnpmfileResult.pnpmfileModule?.fetchers,
        })
      } else if (!optional) {
        throw new PnpmError('PNPMFILE_NOT_FOUND', `pnpmfile at "${file}" is not found`)
      }
    }
  }))

  const mergedFinders: Finders = {}
  const cookedHooks: CookedHooks & Required<Pick<CookedHooks, 'readPackage' | 'beforePacking' | 'preResolution' | 'afterAllResolved' | 'filterLog' | 'updateConfig'>> = {
    readPackage: [],
    beforePacking: [],
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

    // readPackage
    if (fileHooks.readPackage) {
      const fn = fileHooks.readPackage
      const context = createReadPackageHookContext(file, prefix, 'readPackage')
      cookedHooks.readPackage.push(<Pkg extends BaseManifest>(pkg: Pkg, _dir?: string) => fn(pkg, context))
    }

    // beforePacking
    if (fileHooks.beforePacking) {
      const fn = fileHooks.beforePacking
      const context = createReadPackageHookContext(file, prefix, 'beforePacking')
      cookedHooks.beforePacking.push(<Pkg extends BaseManifest>(pkg: Pkg, dir: string) => fn(pkg, dir, context))
    }

    // afterAllResolved
    if (fileHooks.afterAllResolved) {
      const fn = fileHooks.afterAllResolved
      const context = createReadPackageHookContext(file, prefix, 'afterAllResolved')
      cookedHooks.afterAllResolved.push((lockfile) => fn(lockfile, context))
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
  }

  // Process top-level resolvers and fetchers exports
  for (const { resolvers, fetchers } of entries) {
    // Custom resolvers: merge all
    if (resolvers) {
      cookedHooks.customResolvers = cookedHooks.customResolvers ?? []
      cookedHooks.customResolvers.push(...resolvers)
    }

    // Custom fetchers: merge all
    if (fetchers) {
      cookedHooks.customFetchers = cookedHooks.customFetchers ?? []
      cookedHooks.customFetchers.push(...fetchers)
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
  const from = 'pnpmfile'
  return {
    info: (message: string) => {
      hookLogger.info({ message, prefix, hook, from } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    },
    warn: (message: string) => {
      hookLogger.warn({ message, prefix, hook, from } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    },
  }
}
