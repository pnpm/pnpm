import { type AuditReport } from '@pnpm/audit'
import {
  type VulnerabilitySeverity,
  type PackageVulnerability,
  type PackageVulnerabilityAudit,
  type DependenciesField,
} from '@pnpm/types'
import { update } from '@pnpm/plugin-commands-installation'
import semver from 'semver'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { type AuditOptions } from './audit.js'
import { lockfileToPackages } from './lockfileToPackages.js'

interface ExtendedPackageVulnerability {
  vulnerability: PackageVulnerability
  id: number
  semverRange?: semver.Range
}

export interface FixWithUpdateResult {
  // IDs of packages that were fixed
  fixed: number[]
  // IDs of packages that could not be fixed
  remaining: number[]
}

export type FixWithUpdateOptions = AuditOptions & {
  include?: { [dependenciesField in DependenciesField]: boolean }
}

export async function fixWithUpdate (auditReport: AuditReport, opts: FixWithUpdateOptions): Promise<FixWithUpdateResult> {
  const vulnerabilitiesByPackage = new Map<string, ExtendedPackageVulnerability[]>()
  const unfixableVulnerabilities = new Map<string, Set<number>>()
  for (const advisory of Object.values(auditReport.advisories)) {
    let packageVulnerabilities = vulnerabilitiesByPackage.get(advisory.module_name)
    if (!packageVulnerabilities) {
      packageVulnerabilities = []
      vulnerabilitiesByPackage.set(advisory.module_name, packageVulnerabilities)
    }
    const severity: VulnerabilitySeverity = advisory.severity
    const versionRange = advisory.vulnerable_versions
    if (versionRange === '>=0.0.0' || versionRange === '*') {
      // skip unfixable vulnerabilities
      let unfixableForPackage = unfixableVulnerabilities.get(advisory.module_name)
      if (!unfixableForPackage) {
        unfixableForPackage = new Set()
        unfixableVulnerabilities.set(advisory.module_name, unfixableForPackage)
      }
      unfixableForPackage.add(advisory.id)
      continue
    }
    packageVulnerabilities.push({
      vulnerability: {
        versionRange,
        severity,
      },
      id: advisory.id,
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
    getVulnerabilities (): Map<string, PackageVulnerability[]> {
      const allVulnerabilities = new Map<string, PackageVulnerability[]>()
      for (const [pkgName, vulnerabilities] of vulnerabilitiesByPackage) {
        allVulnerabilities.set(pkgName, vulnerabilities.map(v => v.vulnerability))
      }
      return allVulnerabilities
    },
  }

  await update.handler({
    ...opts,
    packageVulnerabilityAudit,
  }, [])

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found after update: Cannot report fixed vulnerabilities`)
  }
  const updatedPackages = lockfileToPackages(lockfile, { include: opts.include })

  const fixed: number[] = []
  const remaining: number[] = []

  for (const [pkgName, vulnerabilities] of vulnerabilitiesByPackage) {
    const updatedVersions = updatedPackages.get(pkgName)
    if (!updatedVersions) {
      fixed.push(...vulnerabilities.map(v => v.id))
      continue
    }
    for (const vulnerability of vulnerabilities) {
      let wasFixed = true
      for (const updatedVersion of updatedVersions) {
        let { semverRange } = vulnerability
        if (!semverRange) {
          semverRange = new semver.Range(vulnerability.vulnerability.versionRange)
          vulnerability.semverRange = semverRange
        }
        if (semver.satisfies(updatedVersion, semverRange)) {
          wasFixed = false
          break
        }
      }
      if (wasFixed) {
        fixed.push(vulnerability.id)
      } else {
        remaining.push(vulnerability.id)
      }
    }
  }

  for (const [pkgName, unfixableIds] of unfixableVulnerabilities) {
    if (updatedPackages.has(pkgName)) {
      remaining.push(...unfixableIds)
    } else {
      fixed.push(...unfixableIds)
    }
  }

  return { fixed, remaining }
}
