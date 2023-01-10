
import path from 'path'
import fs from 'fs'
import { Config } from '@pnpm/config'
import {
  createOrConnectStoreController,
  CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { applyPatchToDep } from '@pnpm/build-modules'
import { PnpmError } from '@pnpm/error'

export type WritePackageOptions = CreateStoreControllerOptions & Pick<Config, 'registries' | 'rootProjectManifest' | 'lockfileDir'> & {
  isCommit?: boolean
  ignorePatches?: boolean
}

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
    requiresBuild: true,
  })

  if (!opts.isCommit && !opts.ignorePatches) {
    const { rootProjectManifest } = opts
    const existedPatchFile = dep.alias && dep.pref && rootProjectManifest?.pnpm?.patchedDependencies?.[`${dep.alias}@${dep.pref}`]
    const lockfileDir = opts.lockfileDir ?? opts.dir ?? process.cwd()
    if (existedPatchFile) {
      const existedPatchFilePath = path.resolve(lockfileDir, existedPatchFile)
      if (!fs.existsSync(existedPatchFilePath)) {
        throw new PnpmError('PATCH_FILE_NOT_FOUND', `Unable to find patch file ${existedPatchFilePath}`)
      }
      applyPatchToDep(dest, existedPatchFilePath)
    }
  }
}
