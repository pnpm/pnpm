import { writeSettings } from '@pnpm/config.writer'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { AuditReport } from '@pnpm/deps.compliance.audit'
import { PnpmError } from '@pnpm/error'
import { update } from '@pnpm/installing.commands'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import type {
  DependenciesField,
  PackageVulnerability,
  PackageVulnerabilityAudit,
  VulnerabilitySeverity,
} from '@pnpm/types'
import semver from 'semver'

import type { AuditOptions } from './audit.js'
import { createMinimumReleaseAgeExcludes } from './fix.js'
import { lockfileToPackages } from './lockfileToPackages.js'
import { createPublishTimesFetcher } from './publishTimes.js'

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
  // Entries added to minimumReleaseAgeExclude
  addedAgeExcludes: string[]
}

export type FixWithUpdateOptions = AuditOptions & {
  include?: { [dependenciesField in DependenciesField]: boolean }
}

interface VulnerabilitiesByPackage {
  fixable: Map<string, ExtendedPackageVulnerability[]>
  unfixable: Map<string, Set<number>>
}

export async function fixWithUpdate (auditReport: AuditReport, opts: FixWithUpdateOptions): Promise<FixWithUpdateResult> {
  const vulnerabilities = groupVulnerabilitiesByPackage(auditReport)
  const addedAgeExcludes = await addMinimumReleaseAgeExcludes(auditReport, opts)
  const updateOpts = { ...opts } as Record<string, unknown>
  if (addedAgeExcludes.length > 0) {
    const existing = (updateOpts.minimumReleaseAgeExclude as string[] | undefined) ?? []
    updateOpts.minimumReleaseAgeExclude = [...existing, ...addedAgeExcludes]
  }

  await update.handler({
    ...updateOpts as FixWithUpdateOptions,
    // The audit command already ran its own prompt to select which
    // vulnerabilities to fix. Forwarding `--interactive` would open the update
    // command's dependency picker on top of that selection.
    interactive: false,
    packageVulnerabilityAudit: createPackageVulnerabilityAudit(vulnerabilities.fixable),
  }, [])

  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found after update: Cannot report fixed vulnerabilities`)
  }
  return {
    ...classifyVulnerabilities(vulnerabilities, lockfileToPackages(lockfile, opts)),
    addedAgeExcludes,
  }
}

function groupVulnerabilitiesByPackage (auditReport: AuditReport): VulnerabilitiesByPackage {
  const fixable = new Map<string, ExtendedPackageVulnerability[]>()
  const unfixable = new Map<string, Set<number>>()
  for (const advisory of Object.values(auditReport.advisories)) {
    const packageVulnerabilities = getOrCreate(fixable, advisory.module_name, () => [])
    const severity: VulnerabilitySeverity = advisory.severity
    const versionRange = advisory.vulnerable_versions
    if (versionRange === '>=0.0.0' || versionRange === '*') {
      // skip unfixable vulnerabilities
      getOrCreate(unfixable, advisory.module_name, () => new Set()).add(advisory.id)
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
  return { fixable, unfixable }
}

function getOrCreate<Key, Value> (map: Map<Key, Value>, key: Key, createValue: () => Value): Value {
  let value = map.get(key)
  if (value === undefined) {
    value = createValue()
    map.set(key, value)
  }
  return value
}

function createPackageVulnerabilityAudit (vulnerabilitiesByPackage: Map<string, ExtendedPackageVulnerability[]>): PackageVulnerabilityAudit {
  return {
    isVulnerable (packageName: string, version: string): boolean {
      const vulnerabilities = vulnerabilitiesByPackage.get(packageName)
      if (!vulnerabilities) return false
      return vulnerabilities.some((vulnerability) => semver.satisfies(version, getSemverRange(vulnerability)))
    },
    getVulnerabilities (): Map<string, PackageVulnerability[]> {
      const allVulnerabilities = new Map<string, PackageVulnerability[]>()
      for (const [pkgName, vulnerabilities] of vulnerabilitiesByPackage) {
        allVulnerabilities.set(pkgName, vulnerabilities.map(v => v.vulnerability))
      }
      return allVulnerabilities
    },
  }
}

function getSemverRange (vulnerability: ExtendedPackageVulnerability): semver.Range {
  vulnerability.semverRange ??= new semver.Range(vulnerability.vulnerability.versionRange)
  return vulnerability.semverRange
}

/**
 * Adds minimum patched versions to minimumReleaseAgeExclude so the resolver
 * can install them even when minimumReleaseAge would otherwise block them.
 */
async function addMinimumReleaseAgeExcludes (auditReport: AuditReport, opts: FixWithUpdateOptions): Promise<string[]> {
  if (!opts.minimumReleaseAge) return []
  const addedAgeExcludes = await createMinimumReleaseAgeExcludes(Object.values(auditReport.advisories), {
    getPublishTimes: opts.getPublishTimes ?? createPublishTimesFetcher(opts),
    minimumReleaseAge: opts.minimumReleaseAge,
  })
  if (addedAgeExcludes.length > 0) {
    await writeSettings({
      addedMinimumReleaseAgeExcludes: addedAgeExcludes,
      rootProjectManifest: opts.rootProjectManifest,
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    })
  }
  return addedAgeExcludes
}

function classifyVulnerabilities (
  vulnerabilities: VulnerabilitiesByPackage,
  updatedPackages: Map<string, Set<string>>
): Pick<FixWithUpdateResult, 'fixed' | 'remaining'> {
  const fixed: number[] = []
  const remaining: number[] = []

  for (const [pkgName, packageVulnerabilities] of vulnerabilities.fixable) {
    const updatedVersions = updatedPackages.get(pkgName)
    for (const vulnerability of packageVulnerabilities) {
      const ids = isStillVulnerable(vulnerability, updatedVersions) ? remaining : fixed
      ids.push(vulnerability.id)
    }
  }

  for (const [pkgName, unfixableIds] of vulnerabilities.unfixable) {
    const ids = updatedPackages.has(pkgName) ? remaining : fixed
    ids.push(...unfixableIds)
  }

  return { fixed, remaining }
}

function isStillVulnerable (vulnerability: ExtendedPackageVulnerability, updatedVersions: Set<string> | undefined): boolean {
  if (!updatedVersions) return false
  return Array.from(updatedVersions).some((updatedVersion) => semver.satisfies(updatedVersion, getSemverRange(vulnerability)))
}
