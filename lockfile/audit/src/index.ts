import path from 'path'
import { PnpmError } from '@pnpm/error'
import { AgentOptions, fetchWithAgent, RetryTimeoutOptions } from '@pnpm/fetch'
import { GetAuthHeader } from '@pnpm/fetching-types'
import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import { lockfileToAuditTree } from './lockfileToAuditTree'
import { AuditReport } from './types'
import { searchForPackages, PackageNode } from '@pnpm/list'
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
  return extendWithDependencyPaths(await (res.json() as Promise<AuditReport>), {
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
  if (Object.keys(advisories).length) {
    const projectDirs = Object.keys(opts.lockfile.importers).map(importer => path.join(opts.lockfileDir, importer))
    for (const { findings, module_name: moduleName } of Object.values(advisories)) {
      for (const finding of findings) {
        finding.paths = await searchPackagePaths(`${moduleName}@${finding.version}`, projectDirs, {
          lockfileDir: opts.lockfileDir,
          depth: Infinity,
          include: opts.include,
        })
      }
    }
  }
  return auditReport
}

async function searchPackagePaths (pkg: string, projectDirs: string[], searchOpts: {
  lockfileDir: string
  depth: number
  include?: { [dependenciesField in DependenciesField]: boolean }
}) {
  const pkgs = await searchForPackages([pkg], projectDirs, searchOpts)
  const paths: string[] = []

  function _walker (packages: PackageNode[], depPath: string) {
    for (const pkg of packages) {
      const nextDepPath = `${depPath}>${pkg.name}@${pkg.version}`
      if (pkg.dependencies?.length) {
        _walker(pkg.dependencies, nextDepPath)
      } else {
        paths.push(nextDepPath)
      }
    }
  }

  for (const pkg of pkgs) {
    _walker([
      ...(pkg.optionalDependencies ?? []),
      ...(pkg.dependencies ?? []),
      ...(pkg.devDependencies ?? []),
      ...(pkg.unsavedDependencies ?? []),
    ], path.relative(searchOpts.lockfileDir, pkg.path) || '.')
  }
  return paths
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
