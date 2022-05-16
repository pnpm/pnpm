import PnpmError from '@pnpm/error'

export class BadEnvVariableError extends PnpmError {
  constructor ({ envName, wantedValue, currentValue }: { envName: string, wantedValue: string, currentValue: string }) {
    super('BAD_ENV_FOUND', `Currently '${envName}' is set to '${wantedValue}'`, {
      hint: `If you want to override the existing ${envName} env variable, use the --force option`,
    })
  }
}

export class BadShellSectionError extends PnpmError {
  public current: string
  public wanted: string
  constructor ({ wanted, current, configFile }: { wanted: string, current: string, configFile: string }) {
    super('BAD_SHELL_SECTION', `The config file at "${configFile} already contains a pnpm section but with other configuration`, {
      hint: 'If you want to override the existing configuration section, use the --force option',
    })
    this.current = current
    this.wanted = wanted
  }
}
