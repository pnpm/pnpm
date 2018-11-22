import { getSaveType } from '@pnpm/utils'
import {
  install,
  installPkgs,
  rebuild,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import requireHooks from '../requireHooks'
import { PnpmOptions } from '../types'
import { recursive } from './recursive'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

/**
 * Perform installation.
 * @example
 *     installCmd([ 'lodash', 'foo' ], { silent: true })
 */
export default async function installCmd (
  input: string[],
  opts: PnpmOptions & {
    allowNew?: boolean,
  },
) {
  // `pnpm install ""` is going to be just `pnpm install`
  input = input.filter(Boolean)

  const prefix = opts.prefix || process.cwd()

  const localPackages = opts.linkWorkspacePackages && opts.workspacePrefix
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspacePrefix))
    : undefined

  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(prefix, opts)
  }
  const store = await createStoreController(opts)
  const installOpts = {
    ...opts,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!localPackages || opts.ignoreScripts,
    localPackages,
    store: store.path,
    storeController: store.ctrl,
  }
  if (!input || !input.length) {
    await install(installOpts)
  } else {
    await installPkgs(Object.assign(installOpts, {
      importers: [
        {
          bin: installOpts.bin,
          prefix: installOpts.prefix,
          targetDependencies: input,
          targetDependenciesField: getSaveType(installOpts),
        },
      ],
    }))
  }

  if (opts.linkWorkspacePackages && opts.workspacePrefix) {
    // TODO: reuse somehow the previous read of packages
    // this is not optimal
    const allWorkspacePkgs = await findWorkspacePackages(opts.workspacePrefix)
    await recursive(allWorkspacePkgs, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      ignoredPackages: new Set([prefix]),
      packageSelectors: [
        {
          matcher: prefix,
          scope: 'dependencies',
          selectBy: 'location',
        },
      ],
    }, 'install', 'install')

    if (opts.ignoreScripts) return

    await rebuild({ ...opts, pending: true } as any) // tslint:disable-line:no-any
  }
}
