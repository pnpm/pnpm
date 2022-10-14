import { Config } from '@pnpm/config'
import {
  createOrConnectStoreController,
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'

export type WritePackageOptions = CreateStoreControllerOptions & Pick<Config, 'registries'>

export async function writePackage (pkg: string, dest: string, opts: WritePackageOptions) {
  const dep = parseWantedDependency(pkg)
  const store = await createOrConnectStoreController({
    ...opts,
    packageImportMethod: 'clone-or-copy',
  })
  const pkgResponse = await store.ctrl.requestPackage(dep, {
    downloadPriority: 1,
    lockfileDir: opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
    registry: (dep.alias && pickRegistryForPackage(opts.registries, dep.alias)) ?? opts.registries.default,
  })
  const filesResponse = await pkgResponse.files!()
  await store.ctrl.importPackage(dest, {
    filesResponse,
    force: true,
  })
}
