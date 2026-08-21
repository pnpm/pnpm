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

type PreferredVersions = Record<string, Record<string, { selectorType: 'version' | 'range', weight: number }>>

const DIRECT_DEP_SELECTOR_WEIGHT = 1000
const EXISTING_VERSION_SELECTOR_WEIGHT = 1_000_000

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

export async function fixWithUpdate (auditReport: AuditReport, opts: FixWithUpdateOptions): Promise<FixWithUpdateResult> {
  const vulnerabilitiesByPackage = new Map<string, ExtendedPackageVulnerability[]>()
  const minPatchedVersionsByPackage = new Map<string, semver.SemVer>()
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
    if (advisory.patched_versions != null) {
      const minimumPatchedVersion = semver.minVersion(advisory.patched_versions)
      if (minimumPatchedVersion != null) {
        const currentMinimum = minPatchedVersionsByPackage.get(advisory.module_name)
        if (currentMinimum == null || semver.gt(minimumPatchedVersion, currentMinimum)) {
          minPatchedVersionsByPackage.set(advisory.module_name, minimumPatchedVersion)
        }
      }
    }
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

  // Add minimum patched versions to minimumReleaseAgeExclude so the resolver
  // can install them even when minimumReleaseAge would otherwise block them.
  const addedAgeExcludes = opts.minimumReleaseAge
    ? await createMinimumReleaseAgeExcludes(Object.values(auditReport.advisories), {
      getPublishTimes: opts.getPublishTimes ?? createPublishTimesFetcher(opts),
      minimumReleaseAge: opts.minimumReleaseAge,
    })
    : []
  const updateOpts = { ...opts } as Record<string, unknown>
  const minPatchedPreferredVersions = createPreferredVersionsFromMinimumPatchedVersions(minPatchedVersionsByPackage)
  if (minPatchedPreferredVersions != null) {
    updateOpts.preferredVersions = mergePreferredVersions(
      updateOpts.preferredVersions as PreferredVersions | undefined,
      minPatchedPreferredVersions
    )
  }
  if (addedAgeExcludes.length > 0) {
    const existing = (updateOpts.minimumReleaseAgeExclude as string[] | undefined) ?? []
    updateOpts.minimumReleaseAgeExclude = [...existing, ...addedAgeExcludes]
    await writeSettings({
      addedMinimumReleaseAgeExcludes: addedAgeExcludes,
      rootProjectManifest: opts.rootProjectManifest,
      rootProjectManifestDir: opts.rootProjectManifestDir,
      workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    })
  }

  await update.handler({
    ...updateOpts as FixWithUpdateOptions,
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

  return { fixed, remaining, addedAgeExcludes }
}

function createPreferredVersionsFromMinimumPatchedVersions (
  minPatchedVersionsByPackage: Map<string, semver.SemVer>
): PreferredVersions | undefined {
  if (minPatchedVersionsByPackage.size === 0) return undefined
  const preferredVersions = Object.create(null) as PreferredVersions
  for (const [packageName, minPatchedVersion] of minPatchedVersionsByPackage) {
    const minPatchedVersionRaw = minPatchedVersion.format()
    preferredVersions[packageName] = {
      [minPatchedVersionRaw]: {
        selectorType: 'version',
        weight: EXISTING_VERSION_SELECTOR_WEIGHT + DIRECT_DEP_SELECTOR_WEIGHT + 1,
      },
      [`<=${minPatchedVersionRaw}`]: {
        selectorType: 'range',
        weight: DIRECT_DEP_SELECTOR_WEIGHT + 1,
      },
    }
  }
  return preferredVersions
}

function mergePreferredVersions (
  basePreferredVersions: PreferredVersions | undefined,
  nextPreferredVersions: PreferredVersions
): PreferredVersions {
  if (basePreferredVersions == null) return nextPreferredVersions
  const mergedPreferredVersions = Object.assign(Object.create(null), basePreferredVersions) as PreferredVersions
  for (const [packageName, selectors] of Object.entries(nextPreferredVersions)) {
    mergedPreferredVersions[packageName] = Object.assign(

      mergedPreferredVersions[packageName],
      selectors
    )
  }
  return mergedPreferredVersions
}
