import { type AuditReport } from '@pnpm/audit'
import { type PackageVulnerability, type PackageVulnerabilityAudit } from '@pnpm/types'
import { update } from '@pnpm/plugin-commands-installation'
import { type AuditOptions } from './audit.js'
import { AUDIT_LEVEL_SEVERITY } from './severity.js'

export async function fixWithUpdate (auditReport: AuditReport, opts: AuditOptions): Promise<void> {
  const vulnerabilitiesByPackage = new Map<string, PackageVulnerability[]>()
  for (const advisory of Object.values(auditReport.advisories)) {
    let packageVulnerabilities = vulnerabilitiesByPackage.get(advisory.module_name)
    if (!packageVulnerabilities) {
      packageVulnerabilities = []
      vulnerabilitiesByPackage.set(advisory.module_name, packageVulnerabilities)
    }
    const severity = AUDIT_LEVEL_SEVERITY[advisory.severity]
    const versionRange = advisory.vulnerable_versions
    if (versionRange === '>=0.0.0') {
      // skip unfixable vulnerabilities
      continue
    }
    packageVulnerabilities.push({
      versionRange,
      severity,
    })
  }

  const packageVulnerabilityAudit: PackageVulnerabilityAudit = {
    isVulnerable (packageName: string, version: string): boolean {
      const vulnerabilities = vulnerabilitiesByPackage.get(packageName)
      if (!vulnerabilities) return false
      // TODO: use semver library to check if version is in vulnerable range
      return true
    },
    getVulnerabilities (packageName: string): PackageVulnerability[] {
      return vulnerabilitiesByPackage.get(packageName) ?? []
    },
  }

  await update.handler({
    ...opts,
    packageVulnerabilityAudit,
  }, ['!*'])
  // The argument '!*' means do not match any package. This limits the update operation
  // to only packages that are found vulnerable by packageVulnerabilityAudit. If we do
  // not provide an argument, all packages will be updated.
}
