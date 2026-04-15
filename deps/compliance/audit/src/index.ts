import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import { type DispatcherOptions, fetchWithDispatcher, type RetryTimeoutOptions } from '@pnpm/network.fetch'
import type { DependenciesField } from '@pnpm/types'
import semver from 'semver'

import { type AuditNode, type AuditTree, lockfileToAuditTree } from './lockfileToAuditTree.js'
import type { AuditAdvisory, AuditFinding, AuditLevelString, AuditReport, AuditVulnerabilityCounts } from './types.js'

export * from './types.js'

// The shape of a single advisory returned by npm's /advisories/bulk endpoint.
// Fields are optional because different registry implementations return different subsets.
interface BulkAdvisory {
  id: number
  url?: string
  title?: string
  severity: AuditLevelString
  vulnerable_versions: string
  patched_versions?: string
  cwe?: string | string[]
  cves?: string[]
  github_advisory_id?: string
  module_name?: string
  created?: string
  updated?: string
  deleted?: boolean | null
  access?: string
  overview?: string
  recommendation?: string
  references?: string
  found_by?: { name: string } | null
  reported_by?: { name: string } | null
  metadata?: AuditAdvisory['metadata'] | null
  npm_advisory_id?: unknown
  findings?: AuditFinding[]
}

type BulkAdvisoriesResponse = Record<string, BulkAdvisory[]>

export async function audit (
  lockfile: LockfileObject,
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    envLockfile?: EnvLockfile | null
    include?: { [dependenciesField in DependenciesField]: boolean }
    lockfileDir: string
    registry: string
    retry?: RetryTimeoutOptions
    timeout?: number
    virtualStoreDirMaxLength: number
  }
): Promise<AuditReport> {
  const auditTree = await lockfileToAuditTree(lockfile, { envLockfile: opts.envLockfile, include: opts.include, lockfileDir: opts.lockfileDir })
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const auditUrl = `${registry}-/npm/v1/security/advisories/bulk`
  const authHeaderValue = getAuthHeader(registry)
  const requestHeaders = {
    'Content-Type': 'application/json',
    ...getAuthHeaders(authHeaderValue),
  }
  const baseOptions = {
    dispatcherOptions: opts.dispatcherOptions ?? {},
    headers: requestHeaders,
    method: 'POST',
    retry: opts.retry,
    timeout: opts.timeout,
  }

  const res = await fetchWithDispatcher(auditUrl, {
    ...baseOptions,
    body: JSON.stringify(buildBulkRequestBody(auditTree)),
  })

  if (res.status === 200) {
    const body = (await res.json()) as BulkAdvisoriesResponse
    return bulkResponseToAuditReport(body, auditTree)
  }

  if (res.status === 404) {
    throw new AuditEndpointNotExistsError(auditUrl)
  }

  throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
}

function buildBulkRequestBody (auditTree: AuditTree): Record<string, string[]> {
  const versionsByName: Record<string, Set<string>> = {}
  const visit = (node: AuditNode): void => {
    if (!node.dependencies) return
    for (const [name, child] of Object.entries(node.dependencies)) {
      if (child.version) {
        if (!versionsByName[name]) versionsByName[name] = new Set()
        versionsByName[name].add(child.version)
      }
      visit(child)
    }
  }
  visit(auditTree)
  const result: Record<string, string[]> = {}
  for (const [name, versions] of Object.entries(versionsByName)) {
    result[name] = [...versions]
  }
  return result
}

interface PathInfo {
  paths: string[]
  dev: boolean
}

type PathIndex = Record<string, Map<string, PathInfo>>

function buildPathIndex (auditTree: AuditTree): PathIndex {
  const index: PathIndex = {}
  const visit = (node: AuditNode, trail: string[]): void => {
    if (!node.dependencies) return
    for (const [name, child] of Object.entries(node.dependencies)) {
      const pathSegments = [...trail, name]
      if (child.version) {
        let byVersion = index[name]
        if (!byVersion) {
          byVersion = new Map()
          index[name] = byVersion
        }
        let info = byVersion.get(child.version)
        if (!info) {
          info = { paths: [], dev: child.dev }
          byVersion.set(child.version, info)
        } else if (!child.dev) {
          info.dev = false
        }
        info.paths.push(pathSegments.join('>'))
      }
      visit(child, pathSegments)
    }
  }
  if (auditTree.dependencies) {
    for (const [importerId, child] of Object.entries(auditTree.dependencies)) {
      visit(child, [importerId])
    }
  }
  return index
}

function bulkResponseToAuditReport (bulk: BulkAdvisoriesResponse, auditTree: AuditTree): AuditReport {
  const pathIndex = buildPathIndex(auditTree)
  const advisories: Record<string, AuditAdvisory> = {}
  const vulnerabilities: AuditVulnerabilityCounts = { info: 0, low: 0, moderate: 0, high: 0, critical: 0 }
  let totalDependencies = 0
  let devDependencies = 0
  for (const byVersion of Object.values(pathIndex)) {
    for (const info of byVersion.values()) {
      totalDependencies += info.paths.length
      if (info.dev) devDependencies += info.paths.length
    }
  }

  for (const [moduleName, packageAdvisories] of Object.entries(bulk)) {
    const byVersion = pathIndex[moduleName]
    for (const adv of packageAdvisories) {
      const findings = buildFindings(adv, byVersion)
      advisories[String(adv.id)] = normalizeAdvisory(adv, moduleName, findings)
      // npm's audit report counts one vulnerability per affected install path.
      const affectedPaths = findings.reduce((sum, f) => sum + f.paths.length, 0) || 1
      vulnerabilities[adv.severity] = (vulnerabilities[adv.severity] ?? 0) + affectedPaths
    }
  }

  return {
    actions: [],
    advisories,
    muted: [],
    metadata: {
      vulnerabilities,
      dependencies: totalDependencies,
      devDependencies,
      optionalDependencies: 0,
      totalDependencies,
    },
  }
}

function buildFindings (adv: BulkAdvisory, byVersion: Map<string, PathInfo> | undefined): AuditFinding[] {
  if (adv.findings && adv.findings.length > 0) return adv.findings
  const findings: AuditFinding[] = []
  if (byVersion) {
    for (const [version, info] of byVersion) {
      if (satisfiesSafe(version, adv.vulnerable_versions)) {
        findings.push({
          version,
          paths: info.paths,
          dev: info.dev,
          optional: false,
          bundled: false,
        })
      }
    }
  }
  if (findings.length === 0) {
    findings.push({ version: '', paths: [], dev: false, optional: false, bundled: false })
  }
  return findings
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
    ...(adv as unknown as AuditAdvisory),
    findings,
    module_name: adv.module_name ?? moduleName,
    cves: adv.cves ?? [],
    cwe: cwe ?? '',
    patched_versions: adv.patched_versions ?? '',
    github_advisory_id: adv.github_advisory_id ?? '',
  }
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
    const message = `The audit endpoint (at ${endpoint}) is doesn't exist.`
    super(
      'AUDIT_ENDPOINT_NOT_EXISTS',
      message,
      {
        hint: 'This issue is probably because you are using a private npm registry and that endpoint doesn\'t have an implementation of audit.',
      }
    )
  }
}
