import { ProjectManifest } from '@pnpm/types'

export type LICENSE_FAILURE_TYPE = 'missing' | 'incompatible'

export interface LicenseInfo {
  license?: string
  licenseFile?: string
}

export interface PackageDetails {
  name: string
  version: string
  path: string
  manifest: ProjectManifest
}

export interface AuditNode {
  version?: string
  integrity?: string
  licenseInfo?: LicenseInfo
  requires?: Record<string, string>
  dependencies?: { [name: string]: AuditNode }
  dev: boolean
}

export type AuditTree = AuditNode & {
  name?: string
  install: string[]
  remove: string[]
  metadata: Object
}

export interface LicenseCompliancePackage {
  name: string
  version: string
}
export interface LicenseComplianceMetadata {
  dependencies: number
  devDependencies: number
  optionalDependencies: number
  totalDependencies: number
}
export interface LicenseComplianceReport {
  licenses: {[license: string]: LicenseCompliancePackage}
  muted: Object[]
  metadata: LicenseComplianceMetadata
}

export interface LicenseCheckResult {
  reason?: LICENSE_FAILURE_TYPE
  pass: boolean
}

export interface Result {
  reason?: LICENSE_FAILURE_TYPE
  license?: string
  repository?: string
}

export type LicensePredicate = (license: string, isFile: boolean) => boolean

export type PackageNamePredicate = (packageName: string) => boolean
