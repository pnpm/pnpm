export interface AuditVulnerabilityCounts {
  info: number
  low: number
  moderate: number
  high: number
  critical: number
}

export interface AuditResolution {
  id: number
  path: string
  dev: boolean
  optional: boolean
  bundled: boolean
}

export interface AuditAction {
  action: string
  module: string
  target: string
  isMajor: boolean
  resolves: AuditResolution[]
}

export interface AuditAdvisory {
  findings: [
    {
      version: string
      paths: string[]
      dev: boolean
      optional: boolean
      bundled: boolean
    },
  ]
  id: number
  created: string
  updated: string
  deleted?: boolean
  title: string
  found_by: {
    name: string
  }
  reported_by: {
    name: string
  }
  module_name: string
  cves: string[]
  vulnerable_versions: string
  patched_versions: string
  overview: string
  recommendation: string
  references: string
  access: string
  severity: string
  cwe: string
  metadata: {
    module_type: string
    exploitability: number
    affected_components: string
  }
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
  actions: AuditAction[]
  advisories: {[id: string]: AuditAdvisory}
  muted: Object[]
  metadata: AuditMetadata
}

export interface AuditActionRecommendation {
  cmd: string
  isBreaking: boolean
  action: AuditAction
}
