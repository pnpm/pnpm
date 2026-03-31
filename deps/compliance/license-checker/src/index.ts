export {
  checkLicensesAfterInstall,
  type CheckAfterInstallOptions,
} from './checkAfterInstall.js'
export {
  checkLicenseCompliance,
  type CheckLicensesResult,
  type LicensePackageInfo,
  type LicenseViolation,
} from './checkLicenses.js'
export {
  extractLicenseIds,
  matchLicenseAgainstPolicy,
  type LicenseMatchResult,
  type MatchPolicyOptions,
} from './spdxMatcher.js'
export {
  collectDirectDeps,
  resolveInclude,
  shouldRunLicenseCheck,
  type IncludeFlags,
} from './utils.js'
