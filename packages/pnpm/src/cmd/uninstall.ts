import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import {
  mutateModules,
  uninstall,
} from 'supi'
import writePkg = require('write-pkg')
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
  if (opts.lockfileDirectory === opts.prefix) {
    const manifest = await uninstall(await readPackageJsonFromDir(opts.prefix), input, uninstallOpts)
    await writePkg(opts.prefix, manifest)
    return
  }
  uninstallOpts['localPackages'] = opts.linkWorkspacePackages && opts.workspacePrefix
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspacePrefix))
    : undefined
  const [{ manifest }] = await mutateModules(
    [
      {
        bin: opts.bin,
        dependencyNames: input,
        manifest: await readPackageJsonFromDir(opts.prefix),
        mutation: 'uninstallSome',
        prefix: opts.prefix,
      },
    ],
    uninstallOpts,
  )
  await writePkg(opts.prefix, manifest)
}
