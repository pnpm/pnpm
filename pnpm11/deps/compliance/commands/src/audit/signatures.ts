import { TABLE_OPTIONS } from '@pnpm/cli.utils'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { lockfileToAuditRequest } from '@pnpm/deps.compliance.audit'
import { type SignaturePackage, type SignatureVerificationResult, verifySignatures } from '@pnpm/deps.security.signatures'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import type { DepPath } from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk from 'chalk'

import type { AuditOptions } from './audit.js'
import { type AuditContext, createAuditNetworkOptions, loadAuditContext } from './auditContext.js'

export async function auditSignatures (opts: AuditOptions): Promise<{ exitCode: number, output: string }> {
  const { envLockfile, include, lockfile } = await loadAuditContext(opts)
  const auditRequest = lockfileToAuditRequest(lockfile, {
    envLockfile,
    include,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
  })
  const packages = signaturePackages({ lockfile, envLockfile, request: auditRequest.request }, opts)
  if (packages.length === 0) {
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
    requireLockfileIntegrity: true,
    noProxy: networkOptions.noProxy,
    retry: networkOptions.retry,
    strictSsl: networkOptions.strictSsl,
    timeout: networkOptions.fetchTimeout,
  })

  return {
    exitCode: result.invalid.length > 0 || result.missing.length > 0 ? 1 : 0,
    output: opts.json ? JSON.stringify(result, null, 2) : renderSignatureVerificationResult(result),
  }
}

type SignatureContext = Pick<AuditContext, 'lockfile' | 'envLockfile'> & { request: Record<string, string[]> }

function signaturePackages (context: SignatureContext, opts: AuditOptions): SignaturePackage[] {
  return Object.entries(context.request).flatMap(([name, versions]) => versions.flatMap((version) => (
    signatureIntegrities(context, `${name}@${version}` as DepPath).map((integrity) => ({
      name,
      version,
      registry: pickRegistryForPackage(opts.registriesByScope, name),
      integrity,
    }))
  )))
}

// Git, tarball, and directory dependencies are keyed by their resolution, not
// by `name@version`, so they find no entry here and are not audited.
function signatureIntegrities (context: SignatureContext, packageId: DepPath): Array<string | undefined> {
  const packages = [context.lockfile.packages?.[packageId], context.envLockfile?.packages[packageId]]
    .filter((pkg) => pkg != null)
  return [...new Set(packages.map(({ resolution }) => (
    'integrity' in resolution && typeof resolution.integrity === 'string' ? resolution.integrity : undefined
  )))]
}

function renderSignatureVerificationResult (result: SignatureVerificationResult): string {
  const lines: string[] = [
    `audited ${result.audited} package${result.audited === 1 ? '' : 's'}`,
    '',
    ...renderVerifiedPackages(result),
    ...renderMissingSignatures(result),
    ...renderInvalidSignatures(result),
  ]

  if (result.audited === 0 && result.invalid.length === 0 && result.missing.length === 0 && result.verified === 0) {
    lines.push('No dependencies were installed from a registry with signing keys')
    lines.push('')
  }

  return lines.join('\n')
}

function renderVerifiedPackages (result: SignatureVerificationResult): string[] {
  if (result.verified === 0) return []
  return [
    `${result.verified} package${result.verified === 1 ? ' has a' : 's have'} ${chalk.bold('verified')} registry signature${result.verified === 1 ? '' : 's'}`,
    '',
  ]
}

function renderMissingSignatures (result: SignatureVerificationResult): string[] {
  if (result.missing.length === 0) return []
  return [
    `${result.missing.length} package${result.missing.length === 1 ? ' is' : 's are'} ${chalk.redBright('missing')} registry signature${result.missing.length === 1 ? '' : 's'} but the registry is providing signing keys:`,
    '',
    table(result.missing.map(({ name, registry, version }) => [chalk.red(`${name}@${version}`), registry]), TABLE_OPTIONS),
    '',
  ]
}

function renderInvalidSignatures (result: SignatureVerificationResult): string[] {
  if (result.invalid.length === 0) return []
  return [
    `${result.invalid.length} package${result.invalid.length === 1 ? ' has an' : 's have'} ${chalk.redBright('invalid')} registry signature${result.invalid.length === 1 ? '' : 's'}:`,
    '',
    table(result.invalid.map(({ name, reason, registry, version }) => [chalk.red(`${name}@${version}`), registry, reason ?? 'Invalid registry signature']), TABLE_OPTIONS),
    '',
    result.invalid.length === 1
      ? 'Someone might have tampered with this package since it was published on the registry!'
      : 'Someone might have tampered with these packages since they were published on the registry!',
    '',
  ]
}
