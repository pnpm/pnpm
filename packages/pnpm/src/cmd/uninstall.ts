import {
  mutateModules,
  uninstall,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import { PnpmOptions } from '../types'

export default async function uninstallCmd (
  input: string[],
  opts: PnpmOptions,
) {
  const store = await createStoreController(opts)
  const uninstallOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })
  if (opts.shrinkwrapDirectory === opts.prefix) {
    return uninstall(input, uninstallOpts)
  }
  uninstallOpts['localPackages'] = opts.linkWorkspacePackages && opts.workspacePrefix
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspacePrefix))
    : undefined
  return mutateModules([
    {
      bin: opts.bin,
      dependencyNames: input,
      mutation: 'uninstallSome',
      prefix: opts.prefix,
    },
  ], uninstallOpts)
}
