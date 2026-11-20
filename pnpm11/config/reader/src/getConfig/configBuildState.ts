import type { ConfigContext, ConfigWithDeprecatedSettings } from '../Config.js'
import type { NpmrcConfigResult } from '../loadNpmrcFiles.js'
import type { CliOptions as SupportedArchitecturesCliOptions } from '../overrideSupportedArchitecturesWithCLI.js'

export type CliOptions = Record<string, unknown> & SupportedArchitecturesCliOptions & { dir?: string, json?: boolean }

export type PnpmConfigInProgress = ConfigWithDeprecatedSettings & ConfigContext

/** What every step of {@link getConfig} reads and writes. */
export interface ConfigBuildState {
  cliOptions: CliOptions
  /** The command line options, camelCased, without the ones left undefined. */
  configFromCliOpts: Record<string, unknown>
  env: Record<string, string | undefined>
  /** The keys set by anything other than the defaults. */
  explicitlySetKeys: Set<string>
  npmrcResult: NpmrcConfigResult
  pnpmConfig: PnpmConfigInProgress
  /**
   * Whether the command line alone set `registry`. A `registry` a yaml
   * declared is explicit too, but it is not the command line, and only the
   * command line outranks the `_auth` environment.
   */
  registrySetOnCommandLine: boolean
  warnings: string[]
}
