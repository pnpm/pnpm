import { PnpmError } from '@pnpm/error'
import { type IgnoredBuilds } from '@pnpm/types'

export class IgnoredBuildsError extends PnpmError {
  constructor (ignoredBuilds: IgnoredBuilds) {
    super('IGNORED_BUILDS', `Ignored build scripts: ${Array.from(ignoredBuilds).join(', ')}`, {
      hint: 'Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.',
    })
  }
}
