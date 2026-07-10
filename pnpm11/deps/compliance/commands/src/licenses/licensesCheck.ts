import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  type LicenseViolation,
  resolveLicensePolicy,
  scanAndCheckLicenses,
} from '@pnpm/deps.compliance.license-checker'
import { PnpmError } from '@pnpm/error'
import type { LicensesConfig } from '@pnpm/types'

import type { LicensesCommandResult } from './LicensesCommandResult.js'
import type { LicensesCommandOptions } from './licensesList.js'
import { renderCheckTable } from './render.js'

export async function licensesCheck (
  opts: LicensesCommandOptions,
  params: string[]
): Promise<LicensesCommandResult> {
  const config = resolveConfig(opts.licenses, params)

  const policy = resolveLicensePolicy(config)
  if (policy == null) {
    return { output: 'No license policy configured. Set licenses.allowed or licenses.disallowed in pnpm-workspace.yaml.', exitCode: 0 }
  }

  // Rootless workspaces have no manifest at the workspace root; the scanner
  // walks the lockfile + store and only uses the root manifest for shallow
  // filtering, where workspace package manifests in selectedProjectsGraph
  // already cover direct deps.
  const manifest = opts.rootProjectManifest ?? {}

  const { result, lockfileMissing } = await scanAndCheckLicenses({
    policy,
    dir: opts.dir,
    lockfileDir: opts.lockfileDir,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir: opts.modulesDir,
    pnpmHomeDir: opts.pnpmHomeDir,
    registries: opts.registries,
    manifest,
    supportedArchitectures: opts.supportedArchitectures,
    selectedProjectsGraph: opts.selectedProjectsGraph,
  })

  if (lockfileMissing) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot check a project without a lockfile`
    )
  }

  if (opts.json) {
    return renderCheckJson(result.violations, result.warnings, result.checkedCount)
  }

  if (result.violations.length === 0 && result.warnings.length === 0) {
    return {
      output: `All ${result.checkedCount} ${result.checkedCount === 1 ? 'package' : 'packages'} passed the license check`,
      exitCode: 0,
    }
  }

  return renderCheckTable(result.violations, result.warnings, result.checkedCount)
}

function resolveConfig (
  licenses: LicensesConfig | undefined,
  params: string[]
): LicensesConfig {
  const config: LicensesConfig = { ...licenses }
  // When called explicitly via CLI, treat 'none' mode as 'loose'
  if (config.mode === 'none' || config.mode === undefined) {
    config.mode = 'loose'
  }
  // Allow overriding environment via positional parameter
  if (params.length > 1) {
    throw new PnpmError(
      'LICENSES_CHECK_TOO_MANY_ARGS',
      `Too many arguments: expected at most one environment, got "${params.join(' ')}"`
    )
  }
  if (params.length === 1) {
    const env = params[0]
    if (env === 'prod' || env === 'dev' || env === 'all') {
      config.environment = env
    } else {
      throw new PnpmError(
        'LICENSES_CHECK_UNKNOWN_ENVIRONMENT',
        `Unknown environment "${env}". Expected one of: prod, dev, all`
      )
    }
  }
  return config
}

function renderCheckJson (
  violations: LicenseViolation[],
  warnings: LicenseViolation[],
  checkedCount: number
): LicensesCommandResult {
  const output = JSON.stringify({
    checkedCount,
    violations,
    warnings,
  }, null, 2)
  return {
    output,
    exitCode: violations.length > 0 ? 1 : 0,
  }
}
