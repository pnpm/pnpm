import path from 'node:path'

import { findDependencyLicenses } from '@pnpm/deps.compliance.license-scanner'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import type { LicensesConfig, ProjectManifest, Registries, SupportedArchitectures } from '@pnpm/types'

import { checkLicenseCompliance, type CheckLicensesResult } from './checkLicenses.js'
import { collectDirectDepKeys } from './directDeps.js'
import type { NormalizedPolicy } from './policy.js'

// Policy-only scan scope, derived from licenses.environment. Deliberately does
// NOT consult transient CLI --prod/--dev flags (that mismatch caused the
// `update --prod` gap). Lives here rather than reusing utils.resolveInclude so
// Task 5 stays additive; utils.resolveInclude is removed in Task 7.
export function includeForEnvironment (
  environment: NonNullable<LicensesConfig['environment']>
): { dependencies: boolean, devDependencies: boolean, optionalDependencies: boolean } {
  if (environment === 'prod') {
    return { dependencies: true, devDependencies: false, optionalDependencies: true }
  }
  if (environment === 'dev') {
    return { dependencies: false, devDependencies: true, optionalDependencies: false }
  }
  return { dependencies: true, devDependencies: true, optionalDependencies: true }
}

export interface ScanOptions {
  policy: NormalizedPolicy
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

export interface ScanResult {
  result: CheckLicensesResult
  lockfileMissing: boolean
}

export async function scanAndCheckLicenses (opts: ScanOptions): Promise<ScanResult> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const [lockfile, storeDir] = await Promise.all([
    readWantedLockfile(lockfileDir, { ignoreIncompatible: true }),
    getStorePath({ pkgRoot: opts.dir, storePath: opts.storeDir, pnpmHomeDir: opts.pnpmHomeDir }),
  ])

  if (lockfile == null) {
    return { result: { violations: [], warnings: [], checkedCount: 0 }, lockfileMissing: true }
  }

  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph).map((p) => getLockfileImporterId(lockfileDir, p))
    : undefined

  let licensePackages = await findDependencyLicenses({
    include: includeForEnvironment(opts.policy.environment ?? 'all'),
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

  if (opts.policy.depth === 'shallow') {
    const directKeys = collectDirectDepKeys(lockfile, includedImporterIds)
    licensePackages = licensePackages.filter((pkg) => directKeys.has(`${pkg.name}@${pkg.version}`))
  }

  return { result: checkLicenseCompliance(licensePackages, opts.policy), lockfileMissing: false }
}
