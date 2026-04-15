import fs from 'node:fs'
import path from 'node:path'

import { lockfileToAuditRequest } from '@pnpm/deps.compliance.audit'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { fixtures } from '@pnpm/test-fixtures'

const f = fixtures(import.meta.dirname)

const REGISTRY = 'https://registry.npmjs.org'

async function writeResponse (lockfileDir: string, filename: string, opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
}): Promise<void> {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (!lockfile) throw new Error(`no lockfile at ${lockfileDir}`)
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const auditRequest = lockfileToAuditRequest(lockfile, { include })
  const res = await fetch(`${REGISTRY}/-/npm/v1/security/advisories/bulk`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(auditRequest.request),
  })
  if (!res.ok) {
    throw new Error(`bulk audit endpoint responded with ${res.status}: ${await res.text()}`)
  }
  const bulkResponse = await res.json()
  fs.writeFileSync(path.join(import.meta.dirname, filename), JSON.stringify(bulkResponse, null, 2))
}

; (async () => {
  await writeResponse(f.find('has-vulnerabilities'), 'dev-vulnerabilities-only-response.json', {
    dev: true,
    production: false,
  })
  await writeResponse(f.find('has-vulnerabilities'), 'all-vulnerabilities-response.json', {})
  await writeResponse(f.find('has-outdated-deps'), 'no-vulnerabilities-response.json', {})
})().catch((err: unknown) => {
  console.error(err)
  process.exitCode = 1
})
