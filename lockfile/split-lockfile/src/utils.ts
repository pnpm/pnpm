import { Lockfile } from '@pnpm/lockfile-types'

type Info = Omit<Lockfile, 'importers' | 'lockfileVersion' | 'packages'>
function del (info: Info, property: keyof Info) {
  if (typeof info[property] === 'undefined') {
    delete info[property]
  }
}

export function pickLockfileInfo (lockfile: Lockfile): Info {
  const info = {
    neverBuiltDependencies: lockfile.neverBuiltDependencies,
    onlyBuiltDependencies: lockfile.onlyBuiltDependencies,
    overrides: lockfile.overrides,
    patchedDependencies: lockfile.patchedDependencies,
    time: lockfile.time,
    packageExtensionsChecksum: lockfile.packageExtensionsChecksum,
  }
  del(info, 'neverBuiltDependencies')
  del(info, 'onlyBuiltDependencies')
  del(info, 'overrides')
  del(info, 'patchedDependencies')
  del(info, 'time')
  del(info, 'packageExtensionsChecksum')
  return info
}
