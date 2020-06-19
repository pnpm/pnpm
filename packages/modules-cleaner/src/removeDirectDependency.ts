import {
  rootLogger,
} from '@pnpm/core-loggers'
import { remove, removeBins } from '@pnpm/remove-bins';
import { DependenciesField } from '@pnpm/types'
import path = require('path')

export default async function removeDirectDependency (
  dependency: {
    dependenciesField?: DependenciesField | undefined,
    name: string,
  },
  opts: {
    binsDir: string,
    dryRun?: boolean,
    modulesDir: string,
    muteLogs?: boolean,
    rootDir: string,
  }
) {
  const results = await Promise.all([
    removeBins(dependency.name, opts),
    !opts.dryRun && remove(path.join(opts.modulesDir, dependency.name)) as any, // tslint:disable-line:no-any
  ])

  const uninstalledPkg = results[0]
  if (!opts.muteLogs) {
    rootLogger.debug({
      prefix: opts.rootDir,
      removed: {
        dependencyType: dependency.dependenciesField === 'devDependencies' && 'dev' ||
          dependency.dependenciesField === 'optionalDependencies' && 'optional' ||
          dependency.dependenciesField === 'dependencies' && 'prod' ||
          undefined,
        name: dependency.name,
        version: uninstalledPkg?.version,
      },
    })
  }
}
