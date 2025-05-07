import { PnpmError } from '@pnpm/error'
import { type AgentOptions, fetchWithAgent, type RetryTimeoutOptions } from '@pnpm/fetch'
import { type GetAuthHeader } from '@pnpm/fetching-types'
import { type LockfileObject } from '@pnpm/lockfile.types'
import { type DependenciesField } from '@pnpm/types'
import { lockfileToAuditTree } from './lockfileToAuditTree'
import { type AuditReport } from './types'

export * from './types'

export async function audit (
  lockfile: LockfileObject,
  getAuthHeader: GetAuthHeader,
  opts: {
    agentOptions?: AgentOptions
    include?: { [dependenciesField in DependenciesField]: boolean }
    lockfileDir: string
    registry: string
    retry?: RetryTimeoutOptions
    timeout?: number
    virtualStoreDirMaxLength: number
  }
): Promise<AuditReport> {
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
  return (res.json() as Promise<AuditReport>)
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
