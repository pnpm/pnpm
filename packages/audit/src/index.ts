import PnpmError from '@pnpm/error'
import { AgentOptions, fetchWithAgent, RetryTimeoutOptions } from '@pnpm/fetch'
import { GetCredentials } from '@pnpm/fetching-types'
import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import lockfileToAuditTree from './lockfileToAuditTree'
import { AuditReport } from './types'

export * from './types'

export default async function audit (
  lockfile: Lockfile,
  getCredentials: GetCredentials,
  opts: {
    agentOptions?: AgentOptions
    include?: { [dependenciesField in DependenciesField]: boolean }
    registry: string
    retry?: RetryTimeoutOptions
    timeout?: number
  }
) {
  const auditTree = lockfileToAuditTree(lockfile, { include: opts.include })
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const auditUrl = `${registry}-/npm/v1/security/audits`
  const credentials = getCredentials(registry)

  const res = await fetchWithAgent(auditUrl, {
    agentOptions: opts.agentOptions ?? {},
    body: JSON.stringify(auditTree),
    headers: {
      'Content-Type': 'application/json',
      ...getAuthHeaders(credentials),
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
  return res.json() as Promise<AuditReport>
}

function getAuthHeaders (
  credentials: {
    authHeaderValue: string | undefined
    alwaysAuth: boolean | undefined
  }
) {
  const headers: { authorization?: string } = {}
  if (credentials.alwaysAuth && credentials.authHeaderValue) {
    headers['authorization'] = credentials.authHeaderValue
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
