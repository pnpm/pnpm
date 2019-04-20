import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getSaveType, safeReadPackageFromDir } from '@pnpm/utils'
import {
  install,
  mutateModules,
  rebuild,
} from 'supi'
import writePkg = require('write-pkg')
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import getPinnedVersion from '../getPinnedVersion'
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
    await install(await readPackageJsonFromDir(opts.prefix), installOpts)
  } else {
    const [{ manifest }] = await mutateModules([
      {
        bin: installOpts.bin,
        dependencySelectors: input,
        manifest: await safeReadPackageFromDir(opts.prefix) || {},
        mutation: 'installSome',
        pinnedVersion: getPinnedVersion(opts),
        prefix: installOpts.prefix,
        targetDependenciesField: getSaveType(installOpts),
      },
    ], installOpts)
    await writePkg(opts.prefix, manifest)
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

    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readPackageJsonFromDir(opts.prefix),
          prefix: opts.prefix,
        },
      ], {
        ...opts,
        pending: true,
      } as any, // tslint:disable-line:no-any
    )
  }
}
