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
