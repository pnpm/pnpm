import fetch from '@pnpm/fetch'
import { Lockfile } from '@pnpm/lockfile-types'
import lockfileToAuditTree from './lockfileToAuditTree'
import { AuditReport } from './types'

export * from './types'

export default async function audit (lockfile: Lockfile, opts: { registry: string }) {
  const auditTree = lockfileToAuditTree(lockfile)
  const registry = opts.registry.endsWith('/') ? opts.registry : `${opts.registry}/`
  const res = await fetch(`${registry}-/npm/v1/security/audits`, {
    body: JSON.stringify(auditTree),
    headers: { 'Content-Type': 'application/json' },
    method: 'post',
  })
  return res.json() as Promise<AuditReport>
}
