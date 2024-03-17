import '@total-typescript/ts-reset'
import path from 'node:path'
import { linkLogger } from '@pnpm/core-loggers'
import symlinkDir from 'symlink-dir'

export { symlinkDirectRootDependency } from './symlinkDirectRootDependency'

export async function symlinkDependency(
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
): Promise<{
    reused: Boolean;
    warn?: string | undefined;
  }> {
  const link = path.join(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  return symlinkDir(dependencyRealLocation, link)
}

export function symlinkDependencySync(
  dependencyRealLocation: string,
  destModulesDir: string,
  importAs: string
): {
    reused: Boolean;
    warn?: string | undefined;
  } {
  const link = path.join(destModulesDir, importAs)
  linkLogger.debug({ target: dependencyRealLocation, link })
  return symlinkDir.sync(dependencyRealLocation, link)
}
