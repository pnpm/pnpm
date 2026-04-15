import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import { type DispatcherOptions, fetchWithDispatcher, type RetryTimeoutOptions } from '@pnpm/network.fetch'
import type { DependenciesField } from '@pnpm/types'

import { lockfileToAuditTree } from './lockfileToAuditTree.js'
import type { AuditReport } from './types.js'

export * from './types.js'

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
  const requestBody = JSON.stringify(auditTree)
  const requestHeaders = {
    'Content-Type': 'application/json',
    ...getAuthHeaders(authHeaderValue),
  }
  const requestOptions = {
    dispatcherOptions: opts.dispatcherOptions ?? {},
    body: requestBody,
    headers: requestHeaders,
    method: 'POST',
    retry: opts.retry,
    timeout: opts.timeout,
  }

  const res = await fetchWithDispatcher(auditUrl, requestOptions)
  if (res.status === 200) {
    return (res.json() as Promise<AuditReport>)
  }

  if (res.status === 404) {
    throw new AuditEndpointNotExistsError(auditUrl)
  }

  throw new PnpmError('AUDIT_BAD_RESPONSE', `The audit endpoint (at ${auditUrl}) responded with ${res.status}: ${await res.text()}`)
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
