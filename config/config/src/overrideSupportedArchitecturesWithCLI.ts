import { type Config } from './Config.js'
import { type types } from './types.js'

const CLI_OPTION_NAMES = ['cpu', 'libc', 'os'] as const satisfies Array<keyof typeof types>
type CliOptionName = typeof CLI_OPTION_NAMES[number]

export type CliOptions = Partial<Record<CliOptionName, string | string[]>>
export type TargetConfig = Pick<Config, 'supportedArchitectures'>

/**
 * If `--cpu`, `--libc`, or `--os` was provided from the command line, override `supportedArchitectures` with them.
 * @param targetConfig - The config object whose `supportedArchitectures` would be overridden.
 * @param cliOptions - The object that contains object
 */
export function overrideSupportedArchitecturesWithCLI (targetConfig: TargetConfig, cliOptions: CliOptions): void {
  for (const key of CLI_OPTION_NAMES) {
    const values = cliOptions[key]
    if (values != null) {
      targetConfig.supportedArchitectures ??= {}
      targetConfig.supportedArchitectures[key] = typeof values === 'string' ? [values] : values
    }
  }
}
