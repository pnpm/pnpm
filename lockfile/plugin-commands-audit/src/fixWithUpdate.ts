import { type AuditReport } from '@pnpm/audit'
import { type VulnerabilitySeverity, type PackageVulnerability, type PackageVulnerabilityAudit } from '@pnpm/types'
import { update } from '@pnpm/plugin-commands-installation'
import semver from 'semver'
import { type AuditOptions } from './audit.js'

interface PackageVulnerabilityWithSemverRange {
  vulnerability: PackageVulnerability
  semverRange?: semver.Range
}

export async function fixWithUpdate (auditReport: AuditReport, opts: AuditOptions): Promise<void> {
  const vulnerabilitiesByPackage = new Map<string, PackageVulnerabilityWithSemverRange[]>()
  for (const advisory of Object.values(auditReport.advisories)) {
    let packageVulnerabilities = vulnerabilitiesByPackage.get(advisory.module_name)
    if (!packageVulnerabilities) {
      packageVulnerabilities = []
      vulnerabilitiesByPackage.set(advisory.module_name, packageVulnerabilities)
    }
    const severity: VulnerabilitySeverity = advisory.severity
    const versionRange = advisory.vulnerable_versions
    if (versionRange === '>=0.0.0') {
      // skip unfixable vulnerabilities
      continue
    }
    packageVulnerabilities.push({
      vulnerability: {
        versionRange,
        severity,
      },
    })
  }

  const packageVulnerabilityAudit: PackageVulnerabilityAudit = {
    isVulnerable (packageName: string, version: string): boolean {
      const vulnerabilities = vulnerabilitiesByPackage.get(packageName)
      if (!vulnerabilities) return false
      for (const vulnerabilityWithRange of vulnerabilities) {
        let { semverRange } = vulnerabilityWithRange
        if (!semverRange) {
          semverRange = new semver.Range(vulnerabilityWithRange.vulnerability.versionRange)
          vulnerabilityWithRange.semverRange = semverRange
        }
        if (semver.satisfies(version, semverRange)) {
          return true
        }
      }
      return false
    },
    getVulnerabilities (packageName: string): PackageVulnerability[] {
      return vulnerabilitiesByPackage.get(packageName)?.map(v => v.vulnerability) ?? []
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
