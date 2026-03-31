import path from 'node:path'

import { findDependencyLicenses } from '@pnpm/deps.compliance.license-scanner'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import type { LicensesConfig, ProjectManifest, Registries, SupportedArchitectures } from '@pnpm/types'

import { checkLicenseCompliance } from './checkLicenses.js'
import { collectDirectDeps, resolveInclude, shouldRunLicenseCheck } from './utils.js'

export interface CheckAfterInstallOptions {
  licenses?: LicensesConfig
  dir: string
  lockfileDir?: string
  storeDir?: string
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  pnpmHomeDir: string
  registries: Registries
  manifest: ProjectManifest
  supportedArchitectures?: SupportedArchitectures
  selectedProjectsGraph?: Record<string, { package: { manifest: ProjectManifest } }>
}

export async function checkLicensesAfterInstall (opts: CheckAfterInstallOptions): Promise<void> {
  if (!shouldRunLicenseCheck(opts.licenses)) {
    return
  }

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: true,
  })
  if (lockfile == null) {
    return
  }

  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })

  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
      .map((projectPath) => getLockfileImporterId(lockfileDir, projectPath))
    : undefined

  let licensePackages = await findDependencyLicenses({
    include: resolveInclude(opts.licenses!.environment ?? 'all'),
    lockfileDir,
    storeDir,
    virtualStoreDir: opts.virtualStoreDir ?? path.join(opts.modulesDir ?? 'node_modules', '.pnpm'),
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir: opts.modulesDir,
    registries: opts.registries,
    wantedLockfile: lockfile,
    manifest: opts.manifest,
    includedImporterIds,
    supportedArchitectures: opts.supportedArchitectures,
  })

  if (opts.licenses!.depth === 'shallow') {
    const directDeps = collectDirectDeps(opts.manifest, opts.selectedProjectsGraph)
    licensePackages = licensePackages.filter((pkg) => directDeps.has(pkg.name))
  }

  const result = checkLicenseCompliance(licensePackages, opts.licenses!)

  if (result.violations.length > 0) {
    const details = result.violations
      .map((v) => `  ${v.packageName}@${v.packageVersion} - ${v.license} - ${v.reason}`)
      .join('\n')
    throw new PnpmError(
      'LICENSE_VIOLATION',
      `${result.violations.length} license violation(s) found:\n${details}`,
      {
        hint: 'Use "pnpm licenses check" for details. ' +
          'To override a specific package, add it to licenses.overrides in pnpm-workspace.yaml.',
      }
    )
  }
}
