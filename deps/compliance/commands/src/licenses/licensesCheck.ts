import path from 'node:path'

import { readProjectManifestOnly } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import {
  checkLicenseCompliance,
  collectDirectDeps,
  type LicenseViolation,
  resolveInclude,
} from '@pnpm/deps.compliance.license-checker'
import { findDependencyLicenses } from '@pnpm/deps.compliance.license-scanner'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import { getStorePath } from '@pnpm/store.path'
import type { LicensesConfig } from '@pnpm/types'
import chalk from 'chalk'

import type { LicensesCommandResult } from './LicensesCommandResult.js'

export type LicensesCheckOptions = Pick<
  Config,
| 'dev'
| 'dir'
| 'lockfileDir'
| 'registries'
| 'optional'
| 'production'
| 'storeDir'
| 'virtualStoreDir'
| 'modulesDir'
| 'pnpmHomeDir'
| 'selectedProjectsGraph'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'supportedArchitectures'
| 'virtualStoreDirMaxLength'
| 'licenses'
| 'workspaceDir'
> &
Partial<Pick<Config, 'userConfig'>> & {
  json?: boolean
}

export async function licensesCheck (
  opts: LicensesCheckOptions,
  params: string[]
): Promise<LicensesCommandResult> {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, {
    ignoreIncompatible: true,
  })
  if (lockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot check a project without a lockfile`
    )
  }

  const config = resolveConfig(opts.licenses, params)

  const include = resolveInclude(config.environment ?? 'all', opts)

  const manifest = await readProjectManifestOnly(opts.dir)

  const includedImporterIds = opts.selectedProjectsGraph
    ? Object.keys(opts.selectedProjectsGraph)
      .map((projectPath) => getLockfileImporterId(opts.lockfileDir ?? opts.dir, projectPath))
    : undefined

  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })

  let licensePackages = await findDependencyLicenses({
    include,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    storeDir,
    virtualStoreDir: opts.virtualStoreDir ?? path.join(opts.modulesDir ?? 'node_modules', '.pnpm'),
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    modulesDir: opts.modulesDir,
    registries: opts.registries,
    wantedLockfile: lockfile,
    manifest,
    includedImporterIds,
    supportedArchitectures: opts.supportedArchitectures,
  })

  if (config.depth === 'shallow') {
    const directDeps = collectDirectDeps(manifest, opts.selectedProjectsGraph)
    licensePackages = licensePackages.filter((pkg) => directDeps.has(pkg.name))
  }

  const result = checkLicenseCompliance(licensePackages, config)

  if (opts.json) {
    return renderCheckJson(result.violations, result.warnings, result.checkedCount)
  }

  if (result.violations.length === 0 && result.warnings.length === 0) {
    return {
      output: `All ${result.checkedCount} packages passed the license check`,
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
  if (params.length > 0) {
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

function renderCheckTable (
  violations: LicenseViolation[],
  warnings: LicenseViolation[],
  checkedCount: number
): LicensesCommandResult {
  const lines: string[] = []

  if (violations.length > 0) {
    lines.push(chalk.red(`${violations.length} license violation(s) found:`))
    lines.push('')
    for (const v of violations) {
      lines.push(`  ${chalk.red('x')} ${v.packageName}@${v.packageVersion} - ${v.license} - ${v.reason}`)
    }
  }

  if (warnings.length > 0) {
    if (lines.length > 0) lines.push('')
    lines.push(chalk.yellow(`${warnings.length} license warning(s):`))
    lines.push('')
    for (const w of warnings) {
      lines.push(`  ${chalk.yellow('!')} ${w.packageName}@${w.packageVersion} - ${w.license} - ${w.reason}`)
    }
  }

  lines.push('')
  lines.push(`Checked ${checkedCount} packages`)

  return {
    output: lines.join('\n'),
    exitCode: violations.length > 0 ? 1 : 0,
  }
}
