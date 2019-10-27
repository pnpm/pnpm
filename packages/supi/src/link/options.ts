import { StoreController } from '@pnpm/store-controller-types'
import {
  DependenciesField,
  ImporterManifest,
  Registries,
} from '@pnpm/types'
import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/utils'
import path = require('path')
import { ReporterFunction } from '../types'

interface StrictLinkOptions {
  bin: string,
  force: boolean,
  forceSharedLockfile: boolean,
  useLockfile: boolean,
  lockfileDir: string,
  pinnedVersion: 'major' | 'minor' | 'patch',
  storeController: StoreController,
  manifest: ImporterManifest,
  registries: Registries,
  storeDir: string,
  reporter: ReporterFunction,
  targetDependenciesField?: DependenciesField,
  workingDir: string,

  hoistPattern: string[] | undefined,
  forceHoistPattern: boolean,

  shamefullyHoist: boolean,
  forceShamefullyHoist: boolean,

  independentLeaves: boolean,
  forceIndependentLeaves: boolean,
}

export type LinkOptions = Partial<StrictLinkOptions> &
  Pick<StrictLinkOptions, 'storeController' | 'manifest'>

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
  const workingDir = opts.workingDir || process.cwd()
  return {
    bin: path.join(workingDir, 'node_modules', '.bin'),
    force: false,
    forceSharedLockfile: false,
    hoistPattern: undefined,
    independentLeaves: false,
    lockfileDir: opts.lockfileDir || workingDir,
    registries: DEFAULT_REGISTRIES,
    shamefullyHoist: false,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    useLockfile: true,
    workingDir,
  } as StrictLinkOptions
}
