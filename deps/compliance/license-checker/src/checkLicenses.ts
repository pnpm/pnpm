import type { DependenciesField, LicensesConfig } from '@pnpm/types'

import { matchLicenseAgainstPolicy } from './spdxMatcher.js'

export interface LicensePackageInfo {
  name: string
  version: string
  license: string
  belongsTo: DependenciesField
}

export interface LicenseViolation {
  packageName: string
  packageVersion: string
  license: string
  belongsTo: DependenciesField
  reason: string
}

export interface CheckLicensesResult {
  violations: LicenseViolation[]
  warnings: LicenseViolation[]
  checkedCount: number
}

export function checkLicenseCompliance (
  packages: LicensePackageInfo[],
  config: LicensesConfig
): CheckLicensesResult {
  const result: CheckLicensesResult = {
    violations: [],
    warnings: [],
    checkedCount: 0,
  }

  if (config.mode === 'none') {
    return result
  }

  const mode = config.mode ?? 'loose'
  const environment = config.environment ?? 'all'
  const filtered = filterByEnvironment(packages, environment)
  const allowed = config.allowed ? new Set(config.allowed) : undefined
  const disallowed = config.disallowed ? new Set(config.disallowed) : undefined
  const overrides = config.overrides ?? {}

  for (const pkg of filtered) {
    result.checkedCount++

    const overrideResult = applyOverride(overrides, pkg.name, pkg.version)
    if (overrideResult === true) {
      continue
    }

    const effectiveLicense = typeof overrideResult === 'string'
      ? overrideResult
      : pkg.license

    const match = matchLicenseAgainstPolicy(effectiveLicense, {
      allowed,
      disallowed,
      mode,
    })

    if (!match.allowed) {
      const violation: LicenseViolation = {
        packageName: pkg.name,
        packageVersion: pkg.version,
        license: effectiveLicense,
        belongsTo: pkg.belongsTo,
        reason: formatReason(match.reason, effectiveLicense),
      }
      // Explicitly disallowed licenses are always violations, even in loose mode.
      // Any other policy failure in loose mode is downgraded to a warning.
      if (mode === 'strict' || match.reason === 'explicitly-disallowed') {
        result.violations.push(violation)
      } else {
        result.warnings.push(violation)
      }
    }
  }

  return result
}

function filterByEnvironment (
  packages: LicensePackageInfo[],
  environment: 'prod' | 'dev' | 'all'
): LicensePackageInfo[] {
  if (environment === 'all') {
    return packages
  }
  if (environment === 'prod') {
    return packages.filter((pkg) => pkg.belongsTo !== 'devDependencies')
  }
  return packages.filter((pkg) => pkg.belongsTo === 'devDependencies')
}

function applyOverride (
  overrides: Record<string, boolean | string>,
  name: string,
  version: string
): boolean | string | undefined {
  const versionKey = `${name}@${version}`
  if (versionKey in overrides) {
    return overrides[versionKey]
  }
  if (name in overrides) {
    return overrides[name]
  }
  return undefined
}

function formatReason (reason: string, license: string): string {
  switch (reason) {
    case 'explicitly-disallowed':
      return `License "${license}" is in the disallowed list`
    case 'not-in-allowed-list':
      return `License "${license}" is not in the allowed list`
    case 'unknown-license':
      return 'Package has no license or an unknown license'
    default:
      return `License "${license}" did not pass the policy check`
  }
}
