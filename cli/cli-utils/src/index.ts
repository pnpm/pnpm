import '@total-typescript/ts-reset'

import chalk from 'chalk'

import {
  checkPackage,
  UnsupportedEngineError,
} from '@pnpm/package-is-installable'
import { logger } from '@pnpm/logger'
import { PnpmError } from '@pnpm/error'
import { packageManager } from '@pnpm/cli-meta'
import { formatWarn } from '@pnpm/default-reporter'
import * as utils from '@pnpm/read-project-manifest'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { getConfig as _getConfig, type CliOptions } from '@pnpm/config'
import type { ActionFailure, Actions, BaseReadProjectManifestResult, Config, ProjectManifest, ReadProjectManifestOpts, ReadProjectManifestResult, RecursiveSummary, SupportedArchitectures } from '@pnpm/types'

export const TABLE_OPTIONS = {
  border: {
    topBody: '─',
    topJoin: '┬',
    topLeft: '┌',
    topRight: '┐',

    bottomBody: '─',
    bottomJoin: '┴',
    bottomLeft: '└',
    bottomRight: '┘',

    bodyJoin: '│',
    bodyLeft: '│',
    bodyRight: '│',

    joinBody: '─',
    joinJoin: '┼',
    joinLeft: '├',
    joinRight: '┤',
  },
  columns: {},
}

for (const [key, value] of Object.entries(TABLE_OPTIONS.border)) {
  // @ts-expect-error
  TABLE_OPTIONS.border[key] = chalk.grey(value)
}

export async function readDepNameCompletions(dir?: string | undefined): Promise<
  {
    name: string
  }[]
> {
  const { manifest } = await readProjectManifest(dir ?? process.cwd())

  return Object.keys(getAllDependenciesFromManifest(manifest)).map(
    (
      name: string
    ): {
      name: string
    } => {
      return {
        name,
      }
    }
  )
}

export async function getConfig(
  cliOptions: CliOptions,
  opts: {
    excludeReporter: boolean
    globalDirShouldAllowWrite?: boolean | undefined
    rcOptionsTypes: Record<string, unknown>
    workspaceDir: string | undefined
    checkUnknownSetting?: boolean | undefined
  }
): Promise<Config> {
  const { config, warnings } = await _getConfig({
    cliOptions,
    globalDirShouldAllowWrite: opts.globalDirShouldAllowWrite ?? false,
    packageManager,
    rcOptionsTypes: opts.rcOptionsTypes,
    workspaceDir: opts.workspaceDir,
    checkUnknownSetting: opts.checkUnknownSetting ?? false,
  })

  config.cliOptions = cliOptions

  if (opts.excludeReporter) {
    delete config.reporter // This is a silly workaround because @pnpm/core expects a function as opts.reporter
  }

  if (warnings.length > 0) {
    console.log(
      warnings
        .map((warning: string): string => {
          return formatWarn(warning)
        })
        .join('\n')
    )
  }

  return config
}

export function packageIsInstallable(
  pkgPath: string,
  pkg: ProjectManifest | undefined,
  opts: {
    engineStrict?: boolean | undefined
    nodeVersion?: string | undefined
    supportedArchitectures?: SupportedArchitectures | undefined
  }
) {
  const pnpmVersion =
    packageManager.name === 'pnpm' ? packageManager.stableVersion : undefined

  const err = checkPackage(pkgPath, pkg, {
    nodeVersion: opts.nodeVersion,
    pnpmVersion,
    supportedArchitectures: opts.supportedArchitectures ?? {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  })

  if (err === null) {
    return
  }

  if (
    (err instanceof UnsupportedEngineError && err.wanted.pnpm) ??
    opts.engineStrict
  ) {
    throw err
  }

  logger.warn({
    message: `Unsupported ${
      err instanceof UnsupportedEngineError ? 'engine' : 'platform'
    }: wanted: ${JSON.stringify(err.wanted)} (current: ${JSON.stringify(err.current)})`,
    prefix: pkgPath,
  })
}

export async function readProjectManifest(
  projectDir: string,
  opts: ReadProjectManifestOpts = {}
): Promise<ReadProjectManifestResult> {
  const { fileName, manifest, writeProjectManifest } =
    await utils.readProjectManifest(projectDir)

  packageIsInstallable(projectDir, manifest, opts)

  return { fileName, manifest, writeProjectManifest }
}

export async function readProjectManifestOnly(
  projectDir: string,
  opts: ReadProjectManifestOpts = {}
): Promise<ProjectManifest> {
  const manifest = await utils.readProjectManifestOnly(projectDir)

  packageIsInstallable(projectDir, manifest, opts)

  return manifest
}

export type TryReadProjectManifestResult = BaseReadProjectManifestResult & {
  manifest: ProjectManifest | undefined
}

export async function tryReadProjectManifest(
  projectDir: string,
  opts: ReadProjectManifestOpts
): Promise<TryReadProjectManifestResult> {
  const { fileName, manifest, writeProjectManifest } =
    await utils.tryReadProjectManifest(projectDir)

  if (manifest == null) {
    return { fileName, manifest, writeProjectManifest }
  }

  packageIsInstallable(projectDir, manifest, opts)

  return { fileName, manifest, writeProjectManifest }
}

class RecursiveFailError extends PnpmError {
  public readonly failures: ActionFailure[]
  public readonly passes: number

  constructor(
    command: string,
    recursiveSummary: RecursiveSummary,
    failures: ActionFailure[]
  ) {
    super(
      'RECURSIVE_FAIL',
      `"${command}" failed in ${failures.length} packages`
    )

    this.failures = failures
    this.passes = Object.values(recursiveSummary).filter(
      ({ status }) => status === 'passed'
    ).length
  }
}

export function throwOnCommandFail(
  command: string,
  recursiveSummary: RecursiveSummary
): void {
  const failures = Object.values(recursiveSummary).filter(
    ({ status }: Actions) => status === 'failure'
  ) as ActionFailure[]
  if (failures.length > 0) {
    throw new RecursiveFailError(command, recursiveSummary, failures)
  }
}

export function docsUrl(cmd: string): string | undefined {
  const version = packageManager.version

  if (typeof version !== 'string') {
    return undefined
  }

  const [pnpmMajorVersion] = version.split('.')
  return `https://pnpm.io/${pnpmMajorVersion ?? ''}.x/cli/${cmd}`
}
