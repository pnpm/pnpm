import PnpmError from '@pnpm/error'

export class BadHomeDirError extends PnpmError {
  constructor ({ wantedDir, currentDir }: { wantedDir: string, currentDir: string }) {
    super('DIFFERENT_HOME_DIR_IS_SET', `Currently 'PNPM_HOME' is set to '${currentDir}'`, {
      hint: 'If you want to override the existing PNPM_HOME env variable, use the --force option',
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
