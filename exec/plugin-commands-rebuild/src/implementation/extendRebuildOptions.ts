import path from 'node:path'

import loadJsonFile from 'load-json-file'

import {
  normalizeRegistries,
  DEFAULT_REGISTRIES,
} from '@pnpm/normalize-registries'

import { getOptionsFromRootManifest } from '@pnpm/config'
import { StrictRebuildOptions } from '@pnpm/types'
import { RebuildOptions } from '.'

async function defaults(opts: RebuildOptions): Promise<StrictRebuildOptions> {
  const packageManager = opts.packageManager ??
    (await loadJsonFile<{ name: string; version: string} >(
      path.join(__dirname, '../../package.json')
    ))

  const dir = opts.dir ?? process.cwd()

  const lockfileDir = opts.lockfileDir ?? dir

  return {
    childConcurrency: 5,
    development: true,
    dir,
    force: false,
    forceSharedLockfile: false,
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
  }
}

export async function extendRebuildOptions(
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
    ...(opts.rootProjectManifest
      ? getOptionsFromRootManifest(
        opts.rootProjectManifestDir,
        opts.rootProjectManifest
      )
      : {}),
  }

  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)

  return extendedOpts
}
