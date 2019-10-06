import {
  mutateModules,
} from 'supi'
import createStoreController from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import readImporterManifest from '../readImporterManifest'
import requireHooks from '../requireHooks'
import { PnpmOptions } from '../types'

export default async function removeCmd (
  input: string[],
  opts: PnpmOptions,
) {
  const store = await createStoreController(opts)
  const removeOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })
  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(opts.lockfileDirectory || opts.prefix, opts)
  }
  removeOpts['localPackages'] = opts.linkWorkspacePackages && opts.workspacePrefix
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspacePrefix, opts))
    : undefined
  const currentManifest = await readImporterManifest(opts.prefix, opts)
  const [mutationResult] = await mutateModules(
    [
      {
        bin: opts.bin,
        dependencyNames: input,
        manifest: currentManifest.manifest,
        mutation: 'uninstallSome',
        prefix: opts.prefix,
      },
    ],
    removeOpts,
  )
  await currentManifest.writeImporterManifest(mutationResult.manifest)
}
