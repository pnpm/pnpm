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
  const res = await fetch(`${registry}-/npm/v1/security/audits`, {
    body: JSON.stringify(auditTree),
    headers: { 'Content-Type': 'application/json' },
    method: 'post',
  })
  return res.json() as Promise<AuditReport>
}
