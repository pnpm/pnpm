export {
  type CheckAfterInstallOptions,
  checkLicensesAfterInstall,
} from './checkAfterInstall.js'
export {
  checkLicenseCompliance,
  type CheckLicensesResult,
  type LicensePackageInfo,
  type LicenseViolation,
} from './checkLicenses.js'
export {
  collectDirectDepKeys,
} from './directDeps.js'
export {
  type NormalizedPolicy,
  resolveLicensePolicy,
} from './policy.js'
export {
  sanitizeForTerminal,
} from './sanitize.js'
export {
  includeForEnvironment,
  scanAndCheckLicenses,
  type ScanOptions,
  type ScanResult,
} from './scan.js'
export {
  extractLicenseIds,
  type LicenseMatchResult,
  matchLicenseAgainstPolicy,
  type MatchPolicyOptions,
} from './spdxMatcher.js'
export {
  collectDirectDeps,
  type IncludeFlags,
  type NormalizedLicenseArgs,
  normalizeLicenseArgs,
  resolveInclude,
  shouldRunLicenseCheck,
} from './utils.js'
