import path from 'path'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { type LogBase } from '@pnpm/logger'
import { normalizeRegistries, DEFAULT_REGISTRIES } from '@pnpm/normalize-registries'
import { type StoreController } from '@pnpm/store-controller-types'
import { type Registries } from '@pnpm/types'
import loadJsonFile from 'load-json-file'

export type StrictRebuildOptions = {
  autoInstallPeers: boolean
  cacheDir: string
  childConcurrency: number
  excludeLinksFromLockfile: boolean
  extraBinPaths: string[]
  extraEnv: Record<string, string>
  lockfileDir: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  preferSymlinkedExecutables?: boolean
  scriptShell?: string
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  scriptsPrependNodePath: boolean | 'warn-only'
  shellEmulator: boolean
  skipIfHasSideEffectsCache?: boolean
  storeDir: string // TODO: remove this property
  storeController: StoreController
  force: boolean
  useLockfile: boolean
  registries: Registries
  dir: string
  pnpmHomeDir: string

  reporter: (logObj: LogBase) => void
  production: boolean
  development: boolean
  optional: boolean
  rawConfig: object
  userConfig: Record<string, string>
  userAgent: string
  packageManager: {
    name: string
    version: string
  }
  unsafePerm: boolean
  pending: boolean
  shamefullyHoist: boolean
  deployAllFiles: boolean
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength: number
  strictStorePkgContentCheck: boolean
} & Pick<Config, 'sslConfigs'>

export type RebuildOptions = Partial<StrictRebuildOptions> &
Pick<StrictRebuildOptions, 'storeDir' | 'storeController'> & Pick<Config, 'rootProjectManifest' | 'rootProjectManifestDir'>

const defaults = async (opts: RebuildOptions): Promise<StrictRebuildOptions> => {
  const packageManager = opts.packageManager ??
    await loadJsonFile<{ name: string, version: string }>(path.join(__dirname, '../../package.json'))!
  const dir = opts.dir ?? process.cwd()
  const lockfileDir = opts.lockfileDir ?? dir
  return {
    childConcurrency: 5,
    development: true,
    dir,
    force: false,
    lockfileDir,
    nodeLinker: 'isolated',
    optional: true,
    packageManager,
    pending: false,
    production: true,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    scriptsPrependNodePath: false,
    shamefullyHoist: false,
    shellEmulator: false,
    sideEffectsCacheRead: false,
    storeDir: opts.storeDir,
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !process.setgid ||
      process.getuid?.() !== 0,
    useLockfile: true,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
  } as StrictRebuildOptions
}

export async function extendRebuildOptions (
  opts: RebuildOptions
): Promise<StrictRebuildOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof RebuildOptions] === undefined) {
        delete opts[key as keyof RebuildOptions]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = {
    ...defaultOpts,
    ...opts,
    storeDir: defaultOpts.storeDir,
    ...(opts.rootProjectManifest ? getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest) : {}),
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}
