import path from 'node:path'

import { DEFAULT_REGISTRIES, normalizeRegistries } from '@pnpm/config.normalize-registries'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import type { LogBase } from '@pnpm/logger'
import type { StoreController } from '@pnpm/store.controller-types'
import type { Registries, RegistryConfig, SupportedArchitectures } from '@pnpm/types'
import { loadJsonFile } from 'load-json-file'

export type StrictBuildOptions = {
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
  configByUri: Record<string, RegistryConfig>
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
  allowBuilds?: Record<string, boolean | string>
  virtualStoreDirMaxLength: number
  peersSuffixMaxLength: number
  strictStorePkgContentCheck: boolean
  fetchFullMetadata?: boolean
  supportedArchitectures?: SupportedArchitectures
} & Pick<Config, 'allowBuilds'>

export type BuildOptions = Partial<StrictBuildOptions> &
Pick<StrictBuildOptions, 'storeDir' | 'storeController'> & Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir'>

const defaults = async (opts: BuildOptions): Promise<StrictBuildOptions> => {
  const packageManager = opts.packageManager ??
    await loadJsonFile<{ name: string, version: string }>(path.join(import.meta.dirname, '../package.json'))!
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
    configByUri: {},
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
  } as StrictBuildOptions
}

export async function extendBuildOptions (
  opts: BuildOptions
): Promise<StrictBuildOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof BuildOptions] === undefined) {
        delete opts[key as keyof BuildOptions]
      }
    }
  }
  const defaultOpts = await defaults(opts)
  const extendedOpts = {
    ...defaultOpts,
    ...opts,
    storeDir: defaultOpts.storeDir,
  }
  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)
  return extendedOpts
}
