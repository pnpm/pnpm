import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type { LicensesConfig, ProjectManifest, Registries, SupportedArchitectures } from '@pnpm/types'

import { resolveLicensePolicy } from './policy.js'
import { sanitizeForTerminal } from './sanitize.js'
import { scanAndCheckLicenses } from './scan.js'

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
  const policy = resolveLicensePolicy(opts.licenses)
  if (policy == null) {
    return
  }

  const { result, lockfileMissing } = await scanAndCheckLicenses({
    policy,
    dir: opts.dir,
    lockfileDir: opts.lockfileDir,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir: opts.modulesDir,
    pnpmHomeDir: opts.pnpmHomeDir,
    registries: opts.registries,
    manifest: opts.manifest,
    supportedArchitectures: opts.supportedArchitectures,
    selectedProjectsGraph: opts.selectedProjectsGraph,
  })

  if (lockfileMissing) {
    globalWarn('License check skipped: no lockfile was found to scan. Run `pnpm install` to generate one.')
    return
  }

  if (result.warnings.length > 0) {
    const details = result.warnings
      .map((w) => `  ${sanitizeForTerminal(w.packageName)}@${sanitizeForTerminal(w.packageVersion)} - ${sanitizeForTerminal(w.license)} - ${sanitizeForTerminal(w.reason)}`)
      .join('\n')
    globalWarn(`${result.warnings.length} license warning(s):\n${details}`)
  }

  if (result.violations.length > 0) {
    const details = result.violations
      .map((v) => `  ${sanitizeForTerminal(v.packageName)}@${sanitizeForTerminal(v.packageVersion)} - ${sanitizeForTerminal(v.license)} - ${sanitizeForTerminal(v.reason)}`)
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
