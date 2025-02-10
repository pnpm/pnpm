import { PnpmError } from '@pnpm/error'

export class IgnoredBuildsError extends PnpmError {
  constructor (ignoredBuilds: string[]) {
    super('IGNORED_BUILDS', `Ignored build scripts: ${ignoredBuilds.join(', ')}`, {
      hint: 'Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.',
    })
  }
}
