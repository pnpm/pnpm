import PnpmError from '@pnpm/error'

export class BadHomeDirError extends PnpmError {
  constructor ({ wantedDir, currentDir }: { wantedDir: string, currentDir: string }) {
    super('DIFFERENT_HOME_DIR_IS_SET', `Currently 'PNPM_HOME' is set to '${currentDir}'`, {
      hint: 'If you want to override the existing PNPM_HOME env variable, use the --force option',
    })
  }
}
