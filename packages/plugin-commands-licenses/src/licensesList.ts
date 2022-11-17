import { readProjectManifestOnly } from '@pnpm/cli-utils'
import { Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { getStorePath } from '@pnpm/store-path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { findDependencyLicenses } from '@pnpm/license-scanner'
import { renderLicences } from './outputRenderer'

export type LicensesCommandOptions = {
  compatible?: boolean
  long?: boolean
  recursive?: boolean
  json?: boolean
} & Pick<
Config,
| 'dev'
| 'dir'
| 'lockfileDir'
| 'registries'
| 'optional'
| 'production'
| 'storeDir'
| 'virtualStoreDir'
| 'modulesDir'
| 'pnpmHomeDir'
> &
Partial<Pick<Config, 'userConfig'>>

export async function licensesList (opts: LicensesCommandOptions) {
  const lockfile = await readWantedLockfile(opts.lockfileDir ?? opts.dir, {
    ignoreIncompatible: true,
  })
  if (lockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot check a project without a lockfile`
    )
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }

  const manifest = await readProjectManifestOnly(opts.dir, {})

  const storeDir = await getStorePath({
    pkgRoot: opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })

  const licensePackages = await findDependencyLicenses({
    include,
    lockfileDir: opts.dir,
    storeDir,
    virtualStoreDir: opts.virtualStoreDir ?? '.',
    modulesDir: opts.modulesDir,
    registries: opts.registries,
    wantedLockfile: lockfile,
    manifest,
  })

  if (licensePackages.length === 0)
    return { output: 'No licenses in packages found', exitCode: 0 }

  return renderLicences(licensePackages, opts)
}
