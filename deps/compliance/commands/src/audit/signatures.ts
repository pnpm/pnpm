import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { lockfileToAuditRequest, type SignaturePackage, type SignatureVerificationResult, verifySignatures } from '@pnpm/deps.compliance.audit'
import { PnpmError } from '@pnpm/error'
import { readEnvLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { table } from '@zkochan/table'
import chalk from 'chalk'

import type { AuditOptions } from './audit.js'

export async function auditSignatures (opts: AuditOptions): Promise<{ exitCode: number, output: string }> {
  const lockfileDir = opts.lockfileDir ?? opts.dir
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  if (lockfile == null) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const envLockfile = await readEnvLockfile(opts.workspaceDir ?? lockfileDir)
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const auditRequest = lockfileToAuditRequest(lockfile, { envLockfile, include })
  const packages: SignaturePackage[] = Object.entries(auditRequest.request).flatMap(([name, versions]) => (
    versions.map((version) => ({ name, registry: opts.registries.default, version }))
  ))
  if (packages.length === 0) {
    throw new PnpmError('AUDIT_NO_PACKAGES', 'No installed packages found to audit')
  }

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri, opts.registries?.default)
  const result = await verifySignatures(packages, getAuthHeader, {
    dispatcherOptions: {
      ca: opts.ca,
      cert: opts.cert,
      httpProxy: opts.httpProxy,
      httpsProxy: opts.httpsProxy,
      key: opts.key,
      localAddress: opts.localAddress,
      maxSockets: opts.maxSockets,
      noProxy: opts.noProxy,
      strictSsl: opts.strictSsl,
      timeout: opts.fetchTimeout,
    },
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })

  return {
    exitCode: result.invalid.length > 0 || result.missing.length > 0 ? 1 : 0,
    output: opts.json ? JSON.stringify(result, null, 2) : renderSignatureVerificationResult(result),
  }
}

function renderSignatureVerificationResult (result: SignatureVerificationResult): string {
  const lines: string[] = []
  lines.push(`audited ${result.audited} package${result.audited === 1 ? '' : 's'}`)
  lines.push('')

  if (result.verified > 0) {
    lines.push(`${result.verified} package${result.verified === 1 ? ' has a' : 's have'} ${chalk.bold('verified')} registry signature${result.verified === 1 ? '' : 's'}`)
    lines.push('')
  }

  if (result.missing.length > 0) {
    lines.push(`${result.missing.length} package${result.missing.length === 1 ? ' is' : 's are'} ${chalk.redBright('missing')} registry signature${result.missing.length === 1 ? '' : 's'} but the registry is providing signing keys:`)
    lines.push('')
    lines.push(table(result.missing.map(({ name, registry, version }) => [chalk.red(`${name}@${version}`), registry]), TABLE_OPTIONS))
    lines.push('')
  }

  if (result.invalid.length > 0) {
    lines.push(`${result.invalid.length} package${result.invalid.length === 1 ? ' has an' : 's have'} ${chalk.redBright('invalid')} registry signature${result.invalid.length === 1 ? '' : 's'}:`)
    lines.push('')
    lines.push(table(result.invalid.map(({ name, reason, registry, version }) => [chalk.red(`${name}@${version}`), registry, reason ?? 'Invalid registry signature']), TABLE_OPTIONS))
    lines.push('')
    lines.push(result.invalid.length === 1
      ? 'Someone might have tampered with this package since it was published on the registry!'
      : 'Someone might have tampered with these packages since they were published on the registry!')
    lines.push('')
  }

  if (result.audited === 0) {
    lines.push('No dependencies were installed from a registry with signing keys')
    lines.push('')
  }

  return lines.join('\n')
}
