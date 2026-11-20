import type { VersionOverride } from '@pnpm/config.parse-overrides'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { ReadPackageHook } from '@pnpm/types'

import type { ProcessedInstallOptions } from './extendInstallOptions.js'

export function createInstallReadPackageHook (
  opts: Pick<ProcessedInstallOptions, 'catalogs' | 'convergeDeclaredRanges' | 'hooks' | 'ignoreCompatibilityDb' | 'ignoredOptionalDependencies' | 'lockfileDir' | 'overridesRootDir' | 'packageExtensions'>,
  parsedOverrides: VersionOverride[]
): ReadPackageHook | undefined {
  return createReadPackageHook({
    catalogs: opts.catalogs,
    ignoreCompatibilityDb: opts.ignoreCompatibilityDb,
    readPackageHook: opts.hooks?.readPackage,
    overrides: parsedOverrides,
    convergeDeclaredRanges: opts.convergeDeclaredRanges,
    lockfileDir: opts.lockfileDir,
    overridesRootDir: opts.overridesRootDir,
    packageExtensions: opts.packageExtensions,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
  })
}
