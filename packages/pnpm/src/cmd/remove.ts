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
    storeController: store.ctrl,
    storeDir: store.dir,
  })
  if (!opts.ignorePnpmfile) {
    opts.hooks = requireHooks(opts.lockfileDir || opts.dir, opts)
  }
  removeOpts['localPackages'] = opts.linkWorkspacePackages && opts.workspaceDir
    ? arrayOfLocalPackagesToMap(await findWorkspacePackages(opts.workspaceDir, opts))
    : undefined
  const currentManifest = await readImporterManifest(opts.dir, opts)
  const [mutationResult] = await mutateModules(
    [
      {
        bin: opts.bin,
        dependencyNames: input,
        manifest: currentManifest.manifest,
        mutation: 'uninstallSome',
        prefix: opts.dir,
      },
    ],
    removeOpts,
  )
  await currentManifest.writeImporterManifest(mutationResult.manifest)
}
