export interface AuditVulnerabilityCounts {
  info: number
  low: number
  moderate: number
  high: number
  critical: number
}

export interface IgnoredAuditVulnerabilityCounts {
  info: number
  low: number
  moderate: number
  high: number
  critical: number
}

export type AuditLevelString = 'info' | 'low' | 'moderate' | 'high' | 'critical'

export type AuditLevelNumber = 0 | 1 | 2 | 3 | 4

export interface AuditFinding {
  version: string
  paths: string[]
  dev: boolean
  optional: boolean
  bundled: boolean
}

export interface AuditAdvisory {
  findings: AuditFinding[]
  id: number
  title: string
  module_name: string
  vulnerable_versions: string
  // Inferred from vulnerable_versions, then narrowed to the version the
  // registry actually publishes. Undefined when inference fails and null when
  // no published version satisfies the inferred range — `audit --fix` and
  // `--ignore-unfixable` treat both as "no fix available".
  patched_versions?: string | null
  // True when `patched_versions` was inferred but then dropped because no
  // published version satisfies it. Distinguishes "no fix shipped yet" from
  // "no fix could be inferred" in the report.
  patched_versions_unpublished?: boolean
  severity: AuditLevelString
  cwe: string
  github_advisory_id: string
  url: string
}

export interface AuditMetadata {
  vulnerabilities: AuditVulnerabilityCounts
  dependencies: number
  devDependencies: number
  optionalDependencies: number
  totalDependencies: number
}

export interface AuditReport {
  advisories: { [id: string]: AuditAdvisory }
  metadata: AuditMetadata
}
