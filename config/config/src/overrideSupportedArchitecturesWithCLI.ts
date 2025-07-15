import { type Config } from './Config'

const CLI_OPTION_NAMES = ['cpu', 'libc', 'os'] as const satisfies Array<keyof Config>
type CliOptionName = typeof CLI_OPTION_NAMES[number]

export type CliOptions = Readonly<Pick<Config, CliOptionName>>
export type TargetConfig = Pick<Config, 'supportedArchitectures'>

/**
 * If `--cpu`, `--libc`, or `--os` was provided from the command line, override `supportedArchitectures` with them.
 * @param config - Both the input and the output.
 */
export function overrideSupportedArchitecturesWithCLI (config: CliOptions & TargetConfig): void {
  for (const key of CLI_OPTION_NAMES) {
    const values = config[key]
    if (values != null) {
      config.supportedArchitectures ??= {}
      config.supportedArchitectures[key] = typeof values === 'string' ? [values] : values
    }
  }
}
