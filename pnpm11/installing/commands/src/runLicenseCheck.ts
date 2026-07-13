import path from 'node:path'

import { tryReadProjectManifest } from '@pnpm/cli.utils'
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

// Global installs (`pnpm add -g` / `pnpm update -g`) split params into
// isolated groups, each installed into its own self-contained directory
// (own lockfile + manifest + node_modules) under the global package dir.
// There is no scannable project at the global dir root, so each group is
// scanned independently right after it is installed.
export async function runLicenseCheckForGlobalInstall (opts: InstallCommandOptions, installDir: string): Promise<void> {
  if (!shouldRunLicenseCheck(opts.licenses)) {
    return
  }
  // installGlobalPackages() writes the group's package.json into installDir;
  // fall back to an empty manifest if it's somehow missing.
  const { manifest } = await tryReadProjectManifest(installDir, opts)
  await checkLicensesAfterInstall({
    licenses: opts.licenses,
    dir: installDir,
    lockfileDir: installDir,
    storeDir: opts.storeDir,
    // The process CWD is not installDir, so the module/virtual-store dirs
    // must be resolved as absolute paths rooted at installDir rather than
    // reusing opts.modulesDir/opts.virtualStoreDir, which are relative to
    // the (non-global) project this InstallCommandOptions was built for.
    modulesDir: path.join(installDir, 'node_modules'),
    virtualStoreDir: path.join(installDir, 'node_modules', '.pnpm'),
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    pnpmHomeDir: opts.pnpmHomeDir,
    registries: opts.registries,
    manifest: manifest ?? {},
    supportedArchitectures: opts.supportedArchitectures,
  })
}
