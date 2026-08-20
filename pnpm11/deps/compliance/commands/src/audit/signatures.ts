import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { lockfileToAuditRequest } from '@pnpm/deps.compliance.audit'
import { type SignatureIssue, type SignaturePackage, type SignatureVerificationResult, sortIssue, verifySignatures } from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { table } from '@zkochan/table'
import chalk from 'chalk'

import type { AuditOptions } from './audit.js'
import { createAuditNetworkOptions, loadAuditContext } from './auditContext.js'

export async function auditSignatures (opts: AuditOptions): Promise<{ exitCode: number, output: string }> {
  const { envLockfile, include, lockfile } = await loadAuditContext(opts)
  const auditRequest = lockfileToAuditRequest(lockfile, { envLockfile, include })
  const packages: SignaturePackage[] = Object.entries(auditRequest.request).flatMap(([name, versions]) => (
    versions.map((version) => ({ name, registry: pickRegistryForPackage(opts.registriesByScope, name), version }))
  ))
  // A lockfile whose references are all unresolvable has no packages to send
  // to the registry, but that is a finding to report, not an empty install.
  if (packages.length === 0 && auditRequest.unresolvable.length === 0) {
    throw new PnpmError('AUDIT_NO_PACKAGES', 'No installed packages found to audit')
  }

  const getAuthHeader = createGetAuthHeaderByURI(opts.configByUri)
  const networkOptions = createAuditNetworkOptions(opts)
  const result = await verifySignatures(packages, getAuthHeader, {
    ca: networkOptions.ca,
    cert: networkOptions.cert,
    configByUri: networkOptions.configByUri,
    httpProxy: networkOptions.httpProxy,
    httpsProxy: networkOptions.httpsProxy,
    key: networkOptions.key,
    localAddress: networkOptions.localAddress,
    maxSockets: networkOptions.maxSockets,
    networkConcurrency: opts.networkConcurrency,
    noProxy: networkOptions.noProxy,
    retry: networkOptions.retry,
    strictSsl: networkOptions.strictSsl,
    timeout: networkOptions.fetchTimeout,
  })

  // A reference the lockfile cannot stand behind has nothing to look up in a
  // registry, so it is reported here rather than left to shrink `audited`.
  if (auditRequest.unresolvable.length > 0) {
    const unresolvableIssues: SignatureIssue[] = auditRequest.unresolvable.map(({ name, depPath }) => ({
      name,
      registry: pickRegistryForPackage(opts.registriesByScope, name),
      version: '',
      reason: `Lockfile entry "${depPath}" has no corresponding package in the lockfile's packages/snapshots section. The lockfile may be broken or tampered with; try reinstalling with --frozen-lockfile to confirm.`,
    }))
    result.audited += unresolvableIssues.length
    result.invalid = [...result.invalid, ...unresolvableIssues].sort(sortIssue)
  }

  return {
    exitCode: result.invalid.length > 0 || result.missing.length > 0 ? 1 : 0,
    output: opts.json ? JSON.stringify(result, null, 2) : renderSignatureVerificationResult(result),
  }
}

// Strings included in the rendered tables originate from the lockfile and the
// registry -- both untrusted input. Strip control characters (C0, DEL, C1) so
// a forged package name or reason cannot inject terminal escape sequences or
// break the table layout.
// eslint-disable-next-line no-control-regex
const CONTROL_CHARS = /[\u0000-\u001F\u007F-\u009F]/g

function sanitizeForTerminal (value: string): string {
  return value.replace(CONTROL_CHARS, '')
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
    lines.push(table(result.missing.map(({ name, registry, version }) => [chalk.red(sanitizeForTerminal(`${name}@${version}`)), sanitizeForTerminal(registry)]), TABLE_OPTIONS))
    lines.push('')
  }

  if (result.invalid.length > 0) {
    lines.push(`${result.invalid.length} package${result.invalid.length === 1 ? ' has an' : 's have'} ${chalk.redBright('invalid')} registry signature${result.invalid.length === 1 ? '' : 's'}:`)
    lines.push('')
    lines.push(table(result.invalid.map(({ name, reason, registry, version }) => [chalk.red(sanitizeForTerminal(`${name}@${version}`)), sanitizeForTerminal(registry), sanitizeForTerminal(reason ?? 'Invalid registry signature')]), TABLE_OPTIONS))
    lines.push('')
    lines.push(result.invalid.length === 1
      ? 'Someone might have tampered with this package since it was published on the registry!'
      : 'Someone might have tampered with these packages since they were published on the registry!')
    lines.push('')
  }

  if (result.audited === 0 && result.invalid.length === 0 && result.missing.length === 0 && result.verified === 0) {
    lines.push('No dependencies were installed from a registry with signing keys')
    lines.push('')
  }

  return lines.join('\n')
}
