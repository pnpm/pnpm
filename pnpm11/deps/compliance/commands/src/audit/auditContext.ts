import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readEnvLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import type { DependenciesField } from '@pnpm/types'
import { pickBy } from 'ramda'

import type { AuditOptions } from './audit.js'

export interface AuditContext {
  envLockfile: EnvLockfile | null
  include: { [dependenciesField in DependenciesField]: boolean }
  lockfile: LockfileObject
  lockfileDir: string
  /**
   * Whether `lockfile` still holds every importer. A lockfile narrowed to
   * some projects cannot decide which workspace-wide ignored advisories are
   * still needed.
   */
  coversEveryImporter: boolean
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
  const auditedLockfile = selectAuditedImporters(lockfile, lockfileDir, opts)
  return {
    envLockfile,
    include: {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    },
    lockfile: auditedLockfile,
    lockfileDir,
    coversEveryImporter: Object.keys(lockfile.importers).every((importerId) => Object.hasOwn(auditedLockfile.importers, importerId)),
  }
}

/**
 * Narrows the lockfile to the importers of the projects selected by
 * `--filter`, `--filter-prod`, or `--workspace-root`. Without a selector, a
 * non-recursive run from a workspace project audits that project's importer,
 * and any other run audits every importer. A selected project without an
 * importer entry is an error: auditing the rest would report it clean.
 */
function selectAuditedImporters (lockfile: LockfileObject, lockfileDir: string, opts: AuditOptions): LockfileObject {
  const selectedImporterIds = selectedAuditImporterIds(lockfileDir, opts)
  if (selectedImporterIds == null) return lockfile
  const missingImporterIds = [...selectedImporterIds].filter((importerId) => !Object.hasOwn(lockfile.importers, importerId)).sort()
  if (missingImporterIds.length > 0) {
    throw new PnpmError('AUDIT_MISSING_IMPORTERS', `${WANTED_LOCKFILE} has no entry for these selected workspace projects: ${missingImporterIds.join(', ')}. Run "pnpm install" to update it.`)
  }
  return {
    ...lockfile,
    importers: pickBy((_, importerId) => selectedImporterIds.has(importerId), lockfile.importers),
  }
}

/**
 * The importer ids the run covers, or `undefined` when it covers the whole
 * lockfile. The current project alone is audited only when the command was
 * not made recursive, so `pnpm -r audit` from a project still covers the
 * workspace.
 */
function selectedAuditImporterIds (lockfileDir: string, opts: AuditOptions): Set<string> | undefined {
  const hasSelector = Boolean(opts.filter?.length || opts.filterProd?.length || opts.workspaceRoot)
  if (hasSelector && opts.selectedProjectsGraph != null) {
    return new Set<string>(
      Object.keys(opts.selectedProjectsGraph).map((projectDir) => getLockfileImporterId(lockfileDir, projectDir))
    )
  }
  const currentImporterId = getLockfileImporterId(lockfileDir, opts.dir)
  if (!hasSelector && !opts.recursive && currentImporterId !== '.') {
    return new Set<string>([currentImporterId])
  }
  return undefined
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
