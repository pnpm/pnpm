import { hookLogger, type HookMessage } from '@pnpm/core-loggers'
import { createHashFromMultipleFiles } from '@pnpm/crypto.hash'
import { PnpmError, redactAndSanitize } from '@pnpm/error'
import type { CustomFetcher, CustomResolver, PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/hooks.types'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { globalWarn } from '@pnpm/logger'
import type { ImportIndexedPackageAsync } from '@pnpm/store.controller-types'
import type { BaseManifest, BeforePackingHook, ReadPackageHook } from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'

import type { HookContext, Hooks } from './Hooks.js'
import { type Finders, type Pnpmfile, requirePnpmfile } from './requirePnpmfile.js'

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- a constraint matching every function type needs any parameters
type Cook<Hook extends (...args: any[]) => any> = (
  arg: Parameters<Hook>[0],
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- the cooked hook forwards whatever extra arguments the caller passes
  ...otherArgs: any[]
) => ReturnType<Hook>

interface PnpmfileEntry {
  path: string
  fallbackPath?: string
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
  /**
   * Whether a `readPackage` hook came from a pnpmfile the checksum does not
   * cover (the global pnpmfile). Its drift is invisible to
   * `pnpmfileChecksum` comparisons, so nothing downstream may treat the
   * checksum as proof the hooks are unchanged.
   */
  untrackedPnpmfileReadPackageHook?: boolean
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
  const pnpmfiles = buildPnpmfileEntries(opts)
  const entries = await loadAllPnpmfiles(pnpmfiles, prefix)
  const mergedFinders = mergeFinders(entries)
  const cookedHooks = cookHooks(entries, prefix)
  attachCustomResolversAndFetchers(entries, cookedHooks)

  return {
    hooks: cookedHooks,
    finders: mergedFinders,
    resolvedPnpmfilePaths: entries.map(({ file }) => file),
  }
}

function buildPnpmfileEntries (opts: {
  globalPnpmfile?: string
  pnpmfiles?: string[]
  tryLoadDefaultPnpmfile?: boolean
}): PnpmfileEntry[] {
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
  if (opts.tryLoadDefaultPnpmfile) {
    pnpmfiles.push({
      path: '.pnpmfile.mjs',
      fallbackPath: '.pnpmfile.cjs',
      includeInChecksum: true,
      optional: true,
    })
  }
  return pnpmfiles
}

async function loadAllPnpmfiles (pnpmfiles: PnpmfileEntry[], prefix: string): Promise<PnpmfileEntryLoaded[]> {
  const entries: PnpmfileEntryLoaded[] = []
  for (const { path: pnpmPath, fallbackPath, includeInChecksum, optional } of pnpmfiles) {
    // eslint-disable-next-line no-await-in-loop -- pnpmfiles must load sequentially to preserve order
    await loadPnpmfileCandidate({ pnpmPath, fallbackPath, includeInChecksum, optional }, prefix, entries)
  }
  return entries
}

async function loadPnpmfileCandidate (
  spec: { pnpmPath: string, fallbackPath?: string, includeInChecksum: boolean, optional?: boolean },
  prefix: string,
  entries: PnpmfileEntryLoaded[]
): Promise<void> {
  const candidates = spec.fallbackPath == null ? [spec.pnpmPath] : [spec.pnpmPath, spec.fallbackPath]
  for (const candidate of candidates) {
    const file = pathAbsolute(candidate, prefix)
    const loadedEntry = entries.find((entry) => entry.file === file)
    if (loadedEntry != null) {
      loadedEntry.includeInChecksum ||= spec.includeInChecksum
      return
    }
    const result = await requirePnpmfile(file, prefix) // eslint-disable-line no-await-in-loop -- pnpmfiles must load in order
    if (result != null) {
      entries.push({
        file,
        includeInChecksum: spec.includeInChecksum,
        hooks: result.pnpmfileModule?.hooks,
        finders: result.pnpmfileModule?.finders,
        resolvers: result.pnpmfileModule?.resolvers,
        fetchers: result.pnpmfileModule?.fetchers,
      })
      return
    }
    if (candidate === (spec.fallbackPath ?? spec.pnpmPath) && !spec.optional) {
      throw new PnpmError('PNPMFILE_NOT_FOUND', `pnpmfile at "${file}" is not found`)
    }
  }
}

function mergeFinders (entries: PnpmfileEntryLoaded[]): Finders {
  const mergedFinders: Finders = {}
  const finderProviders: Record<string, string> = {}
  for (const { file, finders } of entries) {
    if (!finders) continue
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
  return mergedFinders
}

type RequiredCookedHooks = CookedHooks & Required<Pick<CookedHooks, 'readPackage' | 'beforePacking' | 'preResolution' | 'afterAllResolved' | 'filterLog' | 'updateConfig'>>

function cookHooks (entries: PnpmfileEntryLoaded[], prefix: string): CookedHooks {
  const cookedHooks: RequiredCookedHooks = {
    readPackage: [],
    beforePacking: [],
    preResolution: [],
    afterAllResolved: [],
    filterLog: [],
    updateConfig: [],
  }

  if (entries.some((entry) => !entry.includeInChecksum)) {
    cookedHooks.untrackedPnpmfileReadPackageHook = entries.some(
      (entry) => entry.hooks?.readPackage != null && !entry.includeInChecksum
    )
  }
  if (entries.some((entry) => entry.hooks != null)) {
    cookedHooks.calculatePnpmfileChecksum = () => computePnpmfilesChecksum(entries)
  }

  let importProvider: string | undefined
  for (const { hooks, file } of entries) {
    importProvider = applyEntryHooks(hooks ?? {}, file, prefix, cookedHooks, importProvider)
  }
  return cookedHooks
}

async function computePnpmfilesChecksum (entries: PnpmfileEntryLoaded[]): Promise<string> {
  const filesToIncludeInHash: string[] = []
  for (const { includeInChecksum, file } of entries) {
    if (includeInChecksum) {
      filesToIncludeInHash.push(file)
    }
  }
  filesToIncludeInHash.sort()
  return createHashFromMultipleFiles(filesToIncludeInHash)
}

function applyEntryHooks (
  fileHooks: Hooks,
  file: string,
  prefix: string,
  cookedHooks: RequiredCookedHooks,
  importProvider: string | undefined
): string | undefined {
  if (fileHooks.readPackage) {
    const fn = fileHooks.readPackage
    const context = createReadPackageHookContext(file, prefix, 'readPackage')
    cookedHooks.readPackage.push(<Pkg extends BaseManifest>(pkg: Pkg, _dir?: string) => fn(pkg, context))
  }
  if (fileHooks.beforePacking) {
    const fn = fileHooks.beforePacking
    const context = createReadPackageHookContext(file, prefix, 'beforePacking')
    cookedHooks.beforePacking.push(<Pkg extends BaseManifest>(pkg: Pkg, dir: string) => fn(pkg, dir, context))
  }
  if (fileHooks.afterAllResolved) {
    const context = createReadPackageHookContext(file, prefix, 'afterAllResolved')
    cookedHooks.afterAllResolved.push(createCookedAfterAllResolved(fileHooks.afterAllResolved, context))
  }
  if (fileHooks.filterLog) {
    cookedHooks.filterLog.push(fileHooks.filterLog)
  }
  if (fileHooks.updateConfig) {
    cookedHooks.updateConfig.push(createCookedUpdateConfig(fileHooks.updateConfig))
  }
  if (fileHooks.preResolution) {
    const preRes = fileHooks.preResolution
    cookedHooks.preResolution.push((ctx: PreResolutionHookContext) => preRes(ctx, createPreResolutionHookLogger(prefix)))
  }
  return applyImportPackageHook(fileHooks, file, cookedHooks, importProvider)
}

function createCookedUpdateConfig (updateConfig: NonNullable<Hooks['updateConfig']>): Cook<Required<Hooks>['updateConfig']> {
  return (config: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any -- the config object comes from a user pnpmfile and has no fixed shape
    const updated = updateConfig(config)
    return isPromiseLike(updated) ? Promise.resolve(updated).then(assertConfigIsDefined) : assertConfigIsDefined(updated)
  }
}

function createCookedAfterAllResolved (
  afterAllResolved: NonNullable<Hooks['afterAllResolved']>,
  context: HookContext
): Cook<Required<Hooks>['afterAllResolved']> {
  return (lockfile) => {
    const updated = afterAllResolved(lockfile, context)
    return isPromiseLike<LockfileObject>(updated) ? Promise.resolve(updated).then(assertAfterAllResolvedReturn) : assertAfterAllResolvedReturn(updated)
  }
}

function isPromiseLike<Value> (value: unknown): value is PromiseLike<Value> {
  return value != null && (typeof value === 'object' || typeof value === 'function') && typeof (value as PromiseLike<Value>).then === 'function'
}

function applyImportPackageHook (
  fileHooks: Hooks,
  file: string,
  cookedHooks: CookedHooks,
  importProvider: string | undefined
): string | undefined {
  if (!fileHooks.importPackage) return importProvider
  if (importProvider) {
    throw new PnpmError(
      'MULTIPLE_IMPORT_PACKAGE',
      `importPackage hook defined in both ${importProvider} and ${file}`
    )
  }
  cookedHooks.importPackage = fileHooks.importPackage
  globalWarn(
    `The "importPackage" hook (defined in ${redactAndSanitize(file)}) is deprecated and will be removed in the next major version of pnpm. ` +
    'It keeps working until then, but it opts the installation out of the parallel package importer, making it slower.'
  )
  return file
}

function attachCustomResolversAndFetchers (entries: PnpmfileEntryLoaded[], cookedHooks: CookedHooks): void {
  for (const { resolvers, fetchers } of entries) {
    if (resolvers) {
      cookedHooks.customResolvers = cookedHooks.customResolvers ?? []
      cookedHooks.customResolvers.push(...resolvers)
    }
    if (fetchers) {
      cookedHooks.customFetchers = cookedHooks.customFetchers ?? []
      cookedHooks.customFetchers.push(...fetchers)
    }
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
      const logObj: HookMessage = { message, prefix, hook, from }
      hookLogger.info(logObj)
    },
    warn: (message: string) => {
      const logObj: HookMessage & { error?: Error } = { message, prefix, hook, from }
      hookLogger.warn(logObj)
    },
  }
}

function assertConfigIsDefined<Config> (config: Config): Config {
  if (config == null) {
    throw new PnpmError('CONFIG_IS_UNDEFINED', 'The updateConfig hook returned undefined')
  }
  if (typeof config !== 'object' || Array.isArray(config)) {
    throw new PnpmError('CONFIG_IS_UNDEFINED', 'The updateConfig hook did not return a config object')
  }
  return config
}

function assertAfterAllResolvedReturn<Lockfile> (lockfile: Lockfile): Lockfile {
  if (lockfile == null) {
    throw new PnpmError('BAD_AFTER_ALL_RESOLVED_RETURN', 'The afterAllResolved hook returned undefined')
  }
  if (typeof lockfile !== 'object' || Array.isArray(lockfile)) {
    throw new PnpmError('BAD_AFTER_ALL_RESOLVED_RETURN', 'The afterAllResolved hook did not return a lockfile object')
  }
  return lockfile
}
