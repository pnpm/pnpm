import { addDependenciesToPackage } from './addDependenciesToPackage';
import { addDependenciesToLockfile } from './addDependenciesToLockfile';
import { Package } from './Package';
import { Lockfile } from './Lockfile';

export async function add(
  pkg: Package,
  specs: string[],
  opts: {
    // ...
  }
) {
  try {
    await addDependenciesToPackage(pkg, specs, opts);
    await addDependenciesToLockfile(pkg, specs, opts);
  } catch (error) {
    // If the installation fails, do not update the lockfile
    if (error.code === 'EsfwBlock') {
      // Remove the package from the lockfile if it was added
      const lockfile = await Lockfile.find(pkg);
      if (lockfile) {
        lockfile.removePackage(specs[0]);
        await lockfile.write();
      }
    }
    throw error;
  }
}