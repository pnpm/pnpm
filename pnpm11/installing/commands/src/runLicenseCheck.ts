import { readProjectManifestOnly } from '@pnpm/cli.utils'
import { checkLicensesAfterInstall, shouldRunLicenseCheck } from '@pnpm/deps.compliance.license-checker'

import type { InstallCommandOptions } from './install.js'

export async function runLicenseCheck (opts: InstallCommandOptions): Promise<void> {
  if (!shouldRunLicenseCheck(opts.licenses)) {
    return
  }
  const manifest = await readProjectManifestOnly(opts.dir)
  await checkLicensesAfterInstall({
    licenses: opts.licenses,
    dir: opts.dir,
    lockfileDir: opts.lockfileDir,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir: opts.modulesDir,
    pnpmHomeDir: opts.pnpmHomeDir,
    registries: opts.registries,
    manifest,
    supportedArchitectures: opts.supportedArchitectures,
    selectedProjectsGraph: opts.selectedProjectsGraph,
  })
}
