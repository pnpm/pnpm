import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import { type DispatcherOptions, fetchWithDispatcher, type RetryTimeoutOptions } from '@pnpm/network.fetch'
import type { DependenciesField } from '@pnpm/types'
import semver from 'semver'

import {
  type AuditIndexRequest,
  type AuditPathIndex,
  buildAuditPathIndex,
  collectOptionalOnlyDepPaths,
  lockfileToAuditRequest,
  type PathInfo,
} from './lockfileToAuditIndex.js'
import type { AuditAdvisory, AuditFinding, AuditLevelString, AuditReport, AuditVulnerabilityCounts } from './types.js'

export type { AuditIndexRequest, AuditPathIndex, PathInfo } from './lockfileToAuditIndex.js'
export { buildAuditPathIndex, lockfileToAuditRequest } from './lockfileToAuditIndex.js'
export * from './types.js'

// The shape of a single advisory as returned by npm's /advisories/bulk
// endpoint. The two AuditAdvisory fields not populated directly from this
// are derived from it: github_advisory_id from `url` and patched_versions
// from `vulnerable_versions`. findings are built from the lockfile walk.
interface BulkAdvisory {
  id: number
  url?: string
  title?: string
  severity: AuditLevelString
  vulnerable_versions: string
  cwe?: string | string[]
}

type BulkAdvisoriesResponse = Record<string, BulkAdvisory[]>

export async function audit (
  lockfile: LockfileObject,
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    envLockfile?: EnvLockfile | null
    include?: { [dependenciesField in DependenciesField]: boolean }
    registry: string
    retry?: RetryTimeoutOptions
    timeout?: number
  }
): Promise<AuditReport> {
  const depTypes = detectDepTypes(lockfile)
  const optionalOnly = collectOptionalOnlyDepPaths(lockfile, opts.include)
  const auditRequest = lockfileToAuditRequest(lockfile, { envLockfile: opts.envLockfile, include: opts.include, depTypes, optionalOnly })
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const auditUrl = `${registry}-/npm/v1/security/advisories/bulk`
  const authHeaderValue = getAuthHeader(registry)
  const requestHeaders = {
    'Content-Type': 'application/json',
    ...getAuthHeaders(authHeaderValue),
  }

  const res = await fetchWithDispatcher(auditUrl, {
    dispatcherOptions: opts.dispatcherOptions ?? {},
    body: JSON.stringify(auditRequest.request),
    headers: requestHeaders,
    method: 'POST',
    retry: opts.retry,
    timeout: opts.timeout,
  })

  if (res.status === 200) {
    const rawBody = await res.text()
    let body: unknown
    try {
      body = JSON.parse(rawBody)
    } catch (err: unknown) {
      const reason = err instanceof Error ? err.message : String(err)
      throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`)
    }
    if (!isBulkResponseShape(body)) {
      throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) returned an unexpected body. Expected an object keyed by package name; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`)
    }
    const vulnerableNames = new Set(Object.keys(body))
    let auditPathIndex: AuditPathIndex = {}
    if (vulnerableNames.size > 0) {
      auditPathIndex = buildAuditPathIndex(lockfile, vulnerableNames, { envLockfile: opts.envLockfile, include: opts.include, depTypes, optionalOnly })
    }
    return bulkResponseToAuditReport(body, auditRequest, auditPathIndex)
  }

  if (res.status === 404) {
    throw new AuditEndpointNotExistsError(auditUrl)
  }

  throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
}

function bulkResponseToAuditReport (bulk: BulkAdvisoriesResponse, auditRequest: AuditIndexRequest, auditPathIndex: AuditPathIndex): AuditReport {
  // Null-prototype map — the id comes from the registry and could be anything.
  const advisories: Record<string, AuditAdvisory> = Object.create(null)
  const vulnerabilities: AuditVulnerabilityCounts = { info: 0, low: 0, moderate: 0, high: 0, critical: 0 }

  for (const [moduleName, packageAdvisories] of Object.entries(bulk)) {
    const byVersion = auditPathIndex[moduleName]
    for (const adv of packageAdvisories) {
      // Guard against registry-supplied values that could corrupt the report:
      // only accept finite numeric ids and severities from the known set.
      if (typeof adv.id !== 'number' || !Number.isFinite(adv.id)) continue
      if (!isKnownSeverity(adv.severity)) continue
      const findings = buildFindings(adv, byVersion)
      // If no installed version is vulnerable, skip the advisory entirely so
      // we don't report false positives for packages the lockfile doesn't use.
      if (findings.length === 0) continue
      advisories[String(adv.id)] = normalizeAdvisory(adv, moduleName, findings)
      // npm's audit report counts one vulnerability per advisory in the metadata summary
      // when using the bulk endpoint format pnpm expects.
      vulnerabilities[adv.severity] += 1
    }
  }

  return {
    advisories,
    metadata: {
      vulnerabilities,
      dependencies: auditRequest.dependencies,
      devDependencies: auditRequest.devDependencies,
      optionalDependencies: auditRequest.optionalDependencies,
      totalDependencies: auditRequest.totalDependencies,
    },
  }
}

function buildFindings (adv: BulkAdvisory, byVersion: Map<string, PathInfo> | undefined): AuditFinding[] {
  if (byVersion == null) return []
  const findings: AuditFinding[] = []
  for (const [version, info] of byVersion) {
    if (satisfiesSafe(version, adv.vulnerable_versions)) {
      findings.push({
        version,
        paths: info.paths,
        dev: info.dev,
        optional: info.optional,
        bundled: false,
      })
    }
  }
  return findings
}

const KNOWN_SEVERITIES: ReadonlySet<AuditLevelString> = new Set(['info', 'low', 'moderate', 'high', 'critical'])

function isKnownSeverity (severity: unknown): severity is AuditLevelString {
  return typeof severity === 'string' && KNOWN_SEVERITIES.has(severity as AuditLevelString)
}

function isBulkResponseShape (body: unknown): body is BulkAdvisoriesResponse {
  if (typeof body !== 'object' || body === null || Array.isArray(body)) return false
  // Every value must be an array of advisory objects; a null or scalar value
  // would crash `for (const adv of packageAdvisories)` downstream.
  return Object.values(body).every((packageAdvisories) =>
    Array.isArray(packageAdvisories) && packageAdvisories.every((advisory) =>
      typeof advisory === 'object' && advisory !== null && !Array.isArray(advisory) &&
      typeof (advisory as { vulnerable_versions?: unknown }).vulnerable_versions === 'string'
    )
  )
}

function satisfiesSafe (version: string, range: string): boolean {
  try {
    return semver.satisfies(version, range, { includePrerelease: true, loose: true })
  } catch {
    return false
  }
}

function normalizeAdvisory (adv: BulkAdvisory, moduleName: string, findings: AuditFinding[]): AuditAdvisory {
  const cwe = Array.isArray(adv.cwe) ? adv.cwe.join(', ') : adv.cwe
  return {
    findings,
    id: adv.id,
    title: adv.title ?? '',
    module_name: moduleName,
    vulnerable_versions: adv.vulnerable_versions,
    patched_versions: inferPatchedVersions(adv.vulnerable_versions),
    severity: adv.severity,
    cwe: cwe ?? '',
    github_advisory_id: deriveGithubAdvisoryId(adv.url),
    url: adv.url ?? '',
  }
}

function inferPatchedVersions (vulnerableRange: string): string | undefined {
  // Matches `<X.Y.Z` or `<= X.Y.Z` (with optional whitespace after the operator)
  // at the end of the range, optionally preceded by other comparators like
  // `>=0.8.1 <0.28.0`. Returns undefined if the range doesn't have a
  // recognizable upper bound — callers must not confuse that with "no fix".
  const trimmed = vulnerableRange.trim()
  const ltMatch = trimmed.match(/(?:^|\s)<\s*(\d+\.\d+\.\d[\w\-.+]*)\s*$/)
  if (ltMatch) return `>=${ltMatch[1]}`
  const lteMatch = trimmed.match(/(?:^|\s)<=\s*(\d+\.\d+\.\d[\w\-.+]*)\s*$/)
  if (lteMatch) {
    const next = semver.inc(lteMatch[1], 'patch')
    if (next) return `>=${next}`
  }
  return undefined
}

function deriveGithubAdvisoryId (url: string | undefined): string {
  if (!url) return ''
  const match = url.match(/\/(GHSA-[\w-]+)/i)
  return match ? normalizeGhsaId(match[1]) : ''
}

// GHSA identifiers are canonically written with an uppercase `GHSA-` prefix
// and a lowercase hexadecimal-style suffix (e.g. `GHSA-cph5-m8f7-6c5x`).
// Normalize both halves so ignore-list comparisons don't depend on how the
// user (or the advisory url) happens to case the id.
export function normalizeGhsaId (ghsaId: string): string {
  const trimmed = ghsaId.trim()
  const dash = trimmed.indexOf('-')
  if (dash < 0) return trimmed.toUpperCase()
  return trimmed.slice(0, dash).toUpperCase() + trimmed.slice(dash).toLowerCase()
}

interface AuthHeaders {
  authorization?: string
}

function getAuthHeaders (authHeaderValue: string | undefined): AuthHeaders {
  const headers: AuthHeaders = {}
  if (authHeaderValue) {
    headers['authorization'] = authHeaderValue
  }
  return headers
}

export class AuditEndpointNotExistsError extends PnpmError {
  constructor (endpoint: string) {
    const message = `The audit endpoint (at ${endpoint}) doesn't exist.`
    super(
      'AUDIT_ENDPOINT_NOT_EXISTS',
      message,
      {
        hint: 'This issue is probably because you are using a private npm registry and that endpoint doesn\'t have an implementation of audit.',
      }
    )
  }
}
