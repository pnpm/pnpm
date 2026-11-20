import { PnpmError } from '@pnpm/error'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import type { WantedDependency } from '@pnpm/resolving.resolver-base'

export interface NoMatchingVersionErrorOptions {
  wantedDependency: WantedDependency
  packageMeta: PackageMeta
  registry: string
}

export class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  constructor (opts: NoMatchingVersionErrorOptions) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.bareSpecifier ?? ''}`
      : opts.wantedDependency.bareSpecifier!
    super('NO_MATCHING_VERSION', `No matching version found for ${dep} while fetching it from ${opts.registry}`)
    this.packageMeta = opts.packageMeta
  }
}
