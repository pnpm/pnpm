import { StoreController } from '@pnpm/store-controller-types'
import { PackageJson, Registries } from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import { ReporterFunction } from '../types'

export interface LinkOptions {
  bin?: string,
  force?: boolean,
  forceSharedLockfile?: boolean,
  useLockfile?: boolean,
  lockfileDirectory?: string,
  reporter?: ReporterFunction,
  pinnedVersion?: 'major' | 'minor' | 'patch',
  saveProd?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  storeController: StoreController,
  pkg: PackageJson,
  prefix?: string,
  shamefullyFlatten?: boolean,
  independentLeaves?: boolean,
  registries?: Registries,
  store?: string,
}

export type StrictLinkOptions = LinkOptions & {
  bin: string,
  force: boolean,
  forceSharedLockfile: boolean,
  useLockfile: boolean,
  lockfileDirectory: string,
  saveDev: boolean,
  saveOptional: boolean,
  saveProd: boolean,
  prefix: string,
  shamefullyFlatten: boolean,
  independentLeaves: boolean,
  registries: Registries,
  store: string,
}

export async function extendOptions (opts: LinkOptions): Promise<StrictLinkOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = { ...defaultOpts, ...opts, store: defaultOpts.store }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}

async function defaults (opts: LinkOptions) {
  const prefix = opts.prefix || process.cwd()
  return {
    bin: path.join(prefix, 'node_modules', '.bin'),
    force: false,
    forceSharedLockfile: false,
    independentLeaves: false,
    lockfileDirectory: opts.lockfileDirectory || prefix,
    prefix,
    registries: DEFAULT_REGISTRIES,
    shamefullyFlatten: false,
    store: opts.store,
    storeController: opts.storeController,
    useLockfile: true,
  } as StrictLinkOptions
}
