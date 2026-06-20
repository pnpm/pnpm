import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { readEnvLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import type { DependenciesField } from '@pnpm/types'

import type { AuditOptions } from './audit.js'

export interface AuditContext {
  envLockfile: EnvLockfile | null
  include: { [dependenciesField in DependenciesField]: boolean }
  lockfile: LockfileObject
  lockfileDir: string
}

export interface AuditNetworkOptions {
  ca: AuditOptions['ca']
  cert: AuditOptions['cert']
  configByUri: AuditOptions['configByUri']
  fetchTimeout: AuditOptions['fetchTimeout']
  httpProxy: AuditOptions['httpProxy']
  httpsProxy: AuditOptions['httpsProxy']
  key: AuditOptions['key']
  localAddress: AuditOptions['localAddress']
  maxSockets: AuditOptions['maxSockets']
  noProxy: AuditOptions['noProxy']
  retry: {
    factor: AuditOptions['fetchRetryFactor']
    maxTimeout: AuditOptions['fetchRetryMaxtimeout']
    minTimeout: AuditOptions['fetchRetryMintimeout']
    retries: AuditOptions['fetchRetries']
  }
  strictSsl: AuditOptions['strictSsl']
}

export async function loadAuditContext (opts: AuditOptions): Promise<AuditContext> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const envLockfile = await readEnvLockfile(opts.workspaceDir ?? lockfileDir)
  return {
    envLockfile,
    include: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    lockfile,
    lockfileDir,
  }
}

export function createAuditNetworkOptions (opts: AuditOptions): AuditNetworkOptions {
  return {
    ca: opts.ca,
    cert: opts.cert,
    configByUri: opts.configByUri,
    fetchTimeout: opts.fetchTimeout,
    httpProxy: opts.httpProxy,
    httpsProxy: opts.httpsProxy,
    key: opts.key,
    localAddress: opts.localAddress,
    maxSockets: opts.maxSockets,
    noProxy: opts.noProxy,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    strictSsl: opts.strictSsl,
  }
}
