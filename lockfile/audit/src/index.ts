import path from 'path'
import { PnpmError } from '@pnpm/error'
import { type AgentOptions, fetchWithAgent, type RetryTimeoutOptions } from '@pnpm/fetch'
import { type GetAuthHeader } from '@pnpm/fetching-types'
import { type Lockfile } from '@pnpm/lockfile-types'
import { type DependenciesField } from '@pnpm/types'
import { lockfileToAuditTree } from './lockfileToAuditTree'
import { type AuditReport } from './types'
import { searchForPackages, flattenSearchedPackages } from '@pnpm/list'

export * from './types'

export async function audit (
  lockfile: Lockfile,
  getAuthHeader: GetAuthHeader,
  opts: {
    agentOptions?: AgentOptions
    include?: { [dependenciesField in DependenciesField]: boolean }
    lockfileDir: string
    registry: string
    retry?: RetryTimeoutOptions
    timeout?: number
  }
) {
  const auditTree = await lockfileToAuditTree(lockfile, { include: opts.include, lockfileDir: opts.lockfileDir })
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const auditUrl = `${registry}-/npm/v1/security/audits`
  const authHeaderValue = getAuthHeader(registry)

  const res = await fetchWithAgent(auditUrl, {
    agentOptions: opts.agentOptions ?? {},
    body: JSON.stringify(auditTree),
    headers: {
      'Content-Type': 'application/json',
      ...getAuthHeaders(authHeaderValue),
    },
    method: 'post',
    retry: opts.retry,
    timeout: opts.timeout,
  })

  if (res.status === 404) {
    throw new AuditEndpointNotExistsError(auditUrl)
  }

  if (res.status !== 200) {
    throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
  }
  const auditReport = await (res.json() as Promise<AuditReport>)
  return extendWithDependencyPaths(auditReport, {
    lockfile,
    lockfileDir: opts.lockfileDir,
    include: opts.include,
  })
}

function getAuthHeaders (authHeaderValue: string | undefined) {
  const headers: { authorization?: string } = {}
  if (authHeaderValue) {
    headers['authorization'] = authHeaderValue
  }
  return headers
}

async function extendWithDependencyPaths (auditReport: AuditReport, opts: {
  lockfile: Lockfile
  lockfileDir: string
  include?: { [dependenciesField in DependenciesField]: boolean }
}): Promise<AuditReport> {
  const { advisories } = auditReport
  if (!Object.keys(advisories).length) return auditReport
  const projectDirs = Object.keys(opts.lockfile.importers)
    .map((importerId) => path.join(opts.lockfileDir, importerId))
  const searchOpts = {
    lockfileDir: opts.lockfileDir,
    depth: Infinity,
    include: opts.include,
  }
  const _searchPackagePaths = searchPackagePaths.bind(null, searchOpts, projectDirs)
  for (const { findings, module_name: moduleName } of Object.values(advisories)) {
    for (const finding of findings) {
      finding.paths = await _searchPackagePaths(`${moduleName}@${finding.version}`)
    }
  }
  return auditReport
}

async function searchPackagePaths (
  searchOpts: {
    lockfileDir: string
    depth: number
    include?: { [dependenciesField in DependenciesField]: boolean }
  },
  projectDirs: string[],
  pkg: string
) {
  const pkgs = await searchForPackages([pkg], projectDirs, searchOpts)
  return flattenSearchedPackages(pkgs, { lockfileDir: searchOpts.lockfileDir }).map(({ depPath }) => depPath)
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
