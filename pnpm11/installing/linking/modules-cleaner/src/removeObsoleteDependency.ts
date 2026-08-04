import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { rimraf } from '@zkochan/rimraf'
import validateNpmPackageName from 'validate-npm-package-name'

export async function removeObsoleteDependency (modulesDir: string, alias: string): Promise<void> {
  if (!validateNpmPackageName(alias).validForOldPackages) return
  await rimraf(path.join(modulesDir, alias))
  if (alias[0] === '@') {
    try {
      await fs.rmdir(path.join(modulesDir, alias.split('/')[0]))
    } catch (err: unknown) {
      if (
        !util.types.isNativeError(err) ||
        !('code' in err) ||
        err.code !== 'ENOENT' && err.code !== 'ENOTEMPTY'
      ) {
        throw err
      }
    }
  }
}
