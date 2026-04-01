import { readProjectManifestOnly } from '@pnpm/cli.utils'
import { checkLicensesAfterInstall, shouldRunLicenseCheck } from '@pnpm/deps.compliance.license-checker'

import type { InstallCommandOptions } from './install.js'

export async function runLicenseCheck (opts: InstallCommandOptions): Promise<void> {
  if (!shouldRunLicenseCheck(opts.licenses)) {
    return
  }
  // Skip when packages aren't fetched into the store (lockfile/resolution-only
  // operations), since the license scanner needs store index files.
  if (opts.lockfileOnly) {
    return
  }
  const manifest = await readProjectManifestOnly(opts.dir, opts)
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
    dev: opts.dev,
    production: opts.production,
    optional: opts.optional,
  })
}
