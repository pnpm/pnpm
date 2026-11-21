import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import type { LicensesConfig } from '@pnpm/types'

import type { LicenseViolation } from './checkLicenses.js'
import { resolveLicensePolicy } from './policy.js'
import { sanitizeForTerminal } from './sanitize.js'
import { scanAndCheckLicenses, type ScanOptions } from './scan.js'

export type CheckAfterInstallOptions = Omit<ScanOptions, 'policy'> & {
  licenses?: LicensesConfig
}

export async function checkLicensesAfterInstall (opts: CheckAfterInstallOptions): Promise<void> {
  const { licenses, ...scanOpts } = opts
  const policy = resolveLicensePolicy(licenses)
  if (policy == null) {
    return
  }

  const { result, lockfileMissing } = await scanAndCheckLicenses({ ...scanOpts, policy })

  if (lockfileMissing) {
    globalWarn('License check skipped: no lockfile was found to scan. Run `pnpm install` to generate one.')
    return
  }

  if (result.warnings.length > 0) {
    globalWarn(`${result.warnings.length} license warning(s):\n${formatLicenseViolations(result.warnings)}`)
  }

  if (result.violations.length > 0) {
    throw new PnpmError(
      'LICENSE_VIOLATION',
      `${result.violations.length} license violation(s) found:\n${formatLicenseViolations(result.violations)}`,
      {
        hint: 'Use "pnpm licenses check" for details. ' +
          'To override a specific package, add it to licenses.overrides in pnpm-workspace.yaml.',
      }
    )
  }
}

function formatLicenseViolations (violations: LicenseViolation[]): string {
  return violations
    .map((violation) => `  ${sanitizeForTerminal(violation.packageName)}@${sanitizeForTerminal(violation.packageVersion)} - ${sanitizeForTerminal(violation.license)} - ${sanitizeForTerminal(violation.reason)}`)
    .join('\n')
}
