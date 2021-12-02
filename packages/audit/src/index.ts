import PnpmError from '@pnpm/error'
import { AgentOptions, fetchWithAgent, RetryTimeoutOptions } from '@pnpm/fetch'
import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import lockfileToAuditTree from './lockfileToAuditTree'
import { AuditReport } from './types'

export * from './types'

export default async function audit (
  lockfile: Lockfile,
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
  const res = await fetchWithAgent(auditUrl, {
    agentOptions: opts.agentOptions ?? {},
    body: JSON.stringify(auditTree),
    headers: { 'Content-Type': 'application/json' },
    method: 'post',
    retry: opts.retry,
    timeout: opts.timeout,
  })
  if (res.status !== 200) {
    throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
  }
  return res.json() as Promise<AuditReport>
}
