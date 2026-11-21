import type { DependenciesField, LicensesConfig } from '@pnpm/types'

import { type LicenseMatchResult, matchLicenseAgainstPolicy, type MatchPolicyOptions } from './spdxMatcher.js'

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
  const matchLicense = createCachedLicenseMatcher({
    allowed: config.allowed ? new Set(config.allowed) : undefined,
    disallowed: config.disallowed ? new Set(config.disallowed) : undefined,
    mode,
  })
  const overrides = config.overrides ?? {}

  for (const pkg of filterByEnvironment(packages, config.environment ?? 'all')) {
    result.checkedCount++
    const failure = findPolicyFailure(pkg, overrides, matchLicense)
    if (failure == null) continue
    // Explicitly disallowed licenses are always violations, even in loose mode.
    // Any other policy failure in loose mode is downgraded to a warning.
    if (mode === 'strict' || failure.reason === 'explicitly-disallowed') {
      result.violations.push(failure.violation)
    } else {
      result.warnings.push(failure.violation)
    }
  }

  return result
}

function findPolicyFailure (
  pkg: LicensePackageInfo,
  overrides: Record<string, boolean | string>,
  matchLicense: (license: string) => LicenseMatchResult
): { violation: LicenseViolation, reason: LicenseMatchResult['reason'] } | undefined {
  const overrideResult = applyOverride(overrides, pkg.name, pkg.version)
  if (overrideResult === true) return undefined
  const effectiveLicense = typeof overrideResult === 'string' ? overrideResult : pkg.license
  const match = matchLicense(effectiveLicense)
  if (match.allowed) return undefined
  return {
    reason: match.reason,
    violation: {
      packageName: pkg.name,
      packageVersion: pkg.version,
      license: effectiveLicense,
      belongsTo: pkg.belongsTo,
      reason: formatReason(match.reason, effectiveLicense),
    },
  }
}

function createCachedLicenseMatcher (opts: MatchPolicyOptions): (license: string) => LicenseMatchResult {
  const cache = new Map<string, LicenseMatchResult>()
  return (license) => {
    let match = cache.get(license)
    if (match == null) {
      match = matchLicenseAgainstPolicy(license, opts)
      cache.set(license, match)
    }
    return match
  }
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
