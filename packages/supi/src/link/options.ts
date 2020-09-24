import normalizeRegistries, { DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { StoreController } from '@pnpm/store-controller-types'
import {
  DependenciesField,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import { ReporterFunction } from '../types'
import path = require('path')

interface StrictLinkOptions {
  binsDir: string
  force: boolean
  forceSharedLockfile: boolean
  useLockfile: boolean
  lockfileDir: string
  pinnedVersion: 'major' | 'minor' | 'patch'
  storeController: StoreController
  manifest: ProjectManifest
  registries: Registries
  storeDir: string
  reporter: ReporterFunction
  targetDependenciesField?: DependenciesField
  dir: string

  hoistPattern: string[] | undefined
  forceHoistPattern: boolean

  publicHoistPattern: string[] | undefined
  forcePublicHoistPattern: boolean
}

export type LinkOptions =
  & Partial<StrictLinkOptions>
  & Pick<StrictLinkOptions, 'storeController' | 'manifest'>

export async function extendOptions (opts: LinkOptions): Promise<StrictLinkOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key] === undefined) {
        delete opts[key]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = { ...defaultOpts, ...opts, storeDir: defaultOpts.storeDir }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}

async function defaults (opts: LinkOptions) {
  const dir = opts.dir ?? process.cwd()
  return {
    binsDir: path.join(dir, 'node_modules', '.bin'),
    dir,
    force: false,
    forceSharedLockfile: false,
    hoistPattern: undefined,
    lockfileDir: opts.lockfileDir ?? dir,
    registries: DEFAULT_REGISTRIES,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    useLockfile: true,
  } as StrictLinkOptions
}
