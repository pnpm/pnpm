import path from 'path'
import { normalizeRegistries, DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { type StoreController } from '@pnpm/store-controller-types'
import {
  type DependenciesField,
  type ProjectManifest,
  type Registries,
} from '@pnpm/types'
import { type ReporterFunction } from '../types'

interface StrictLinkOptions {
  autoInstallPeers: boolean
  binsDir: string
  excludeLinksFromLockfile: boolean
  force: boolean
  useLockfile: boolean
  lockfileDir: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  pinnedVersion: 'major' | 'minor' | 'patch'
  storeController: StoreController
  manifest: ProjectManifest
  registries: Registries
  storeDir: string
  reporter: ReporterFunction
  targetDependenciesField?: DependenciesField
  dir: string
  preferSymlinkedExecutables: boolean

  hoistPattern: string[] | undefined
  forceHoistPattern: boolean

  publicHoistPattern: string[] | undefined
  forcePublicHoistPattern: boolean

  useGitBranchLockfile: boolean
  mergeGitBranchLockfiles: boolean
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength: number
}

export type LinkOptions =
  & Partial<StrictLinkOptions>
  & Pick<StrictLinkOptions, 'storeController' | 'manifest'>

export async function extendOptions (opts: LinkOptions): Promise<StrictLinkOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof LinkOptions] === undefined) {
        delete opts[key as keyof LinkOptions]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = { ...defaultOpts, ...opts, storeDir: defaultOpts.storeDir }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}

async function defaults (opts: LinkOptions): Promise<StrictLinkOptions> {
  const dir = opts.dir ?? process.cwd()
  return {
    binsDir: path.join(dir, 'node_modules', '.bin'),
    dir,
    force: false,
    hoistPattern: undefined,
    lockfileDir: opts.lockfileDir ?? dir,
    nodeLinker: 'isolated',
    registries: DEFAULT_REGISTRIES,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    useLockfile: true,
    virtualStoreDirMaxLength: 120,
  } as StrictLinkOptions
}
