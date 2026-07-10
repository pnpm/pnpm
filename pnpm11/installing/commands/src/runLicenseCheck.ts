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
  // Rootless workspaces have no manifest at the workspace root; the scanner
  // walks the lockfile + store and only uses the root manifest for shallow
  // filtering, where workspace package manifests in selectedProjectsGraph
  // already cover direct deps.
  const manifest = opts.rootProjectManifest ?? {}
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
