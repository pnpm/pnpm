import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'

export default class LockfileMissingDependencyError extends PnpmError {
  constructor (depPath: string) {
    const message = `Broken lockfile: no entry for '${depPath}' in ${WANTED_LOCKFILE}`
    super('LOCKFILE_MISSING_DEPENDENCY', message, {
      hint: 'This issue is probably caused by a badly resolved merge conflict.\n' +
        'To fix the lockfile, run \'pnpm install --no-frozen-lockfile\'.',
    })
  }
}
