import path from 'node:path'
import {
  normalizeRegistries,
  DEFAULT_REGISTRIES,
} from '@pnpm/normalize-registries'
import type {
  LinkOptions,
  StrictLinkOptions,
} from '@pnpm/types'

export async function extendOptions(
  opts: LinkOptions
): Promise<StrictLinkOptions> {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof LinkOptions] === undefined) {
        delete opts[key as keyof LinkOptions]
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

async function defaults(opts: LinkOptions): Promise<StrictLinkOptions> {
  const dir = opts.dir ?? process.cwd()

  return {
    binsDir: path.join(dir, 'node_modules', '.bin'),
    dir,
    force: false,
    forceSharedLockfile: false,
    hoistPattern: undefined,
    lockfileDir: opts.lockfileDir ?? dir,
    nodeLinker: 'isolated',
    registries: DEFAULT_REGISTRIES,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    useLockfile: true,
  }
}
