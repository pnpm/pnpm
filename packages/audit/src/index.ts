import PnpmError from '@pnpm/error'
import fetch from '@pnpm/fetch'
import { Lockfile } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'
import lockfileToAuditTree from './lockfileToAuditTree'
import { AuditReport } from './types'

export * from './types'

export default async function audit (
  lockfile: Lockfile,
  opts: {
    include?: { [dependenciesField in DependenciesField]: boolean },
    registry: string,
  },
) {
  const auditTree = lockfileToAuditTree(lockfile, { include: opts.include })
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const auditUrl = `${registry}-/npm/v1/security/audits`
  const res = await fetch(auditUrl, {
    body: JSON.stringify(auditTree),
    headers: { 'Content-Type': 'application/json' },
    method: 'post',
  })
  if (res.status !== 200) {
    throw new PnpmError('AUDIT_SERVER_ERROR', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
  }
  return res.json() as Promise<AuditReport>
}
