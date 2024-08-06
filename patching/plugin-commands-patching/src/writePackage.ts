import { type Config } from '@pnpm/config'
import {
  createOrConnectStoreController,
  type CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import type { ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'

export type WritePackageOptions = CreateStoreControllerOptions & Pick<Config, 'registries'>

export async function writePackage (dep: ParseWantedDependencyResult, dest: string, opts: WritePackageOptions): Promise<void> {
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
  const { files } = await pkgResponse.fetching!()
  await store.ctrl.importPackage(dest, {
    filesResponse: files,
    force: true,
  })
}
