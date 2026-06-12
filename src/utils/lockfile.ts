import { Lockfile } from './Lockfile';

export async function removePackageFromLockfile(
  pkg: Package,
  packageName: string
) {
  const lockfile = await Lockfile.find(pkg);
  if (lockfile) {
    lockfile.removePackage(packageName);
    await lockfile.write();
  }
}