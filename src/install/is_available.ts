import semver = require('semver')
import path = require('path')
import fs = require('mz/fs')
import {PackageSpec} from '../install'
import {Package} from '../api/init_cmd'

/*
 * Check if a module exists (eg, `node_modules/node-pre-gyp`). This is the case when
 * it's part of bundleDependencies.
 *
 * This check is also responsible for stopping `pnpm i lodash` from doing anything when
 * 'node_modules/lodash' already exists.
 *
 *     spec = { name: 'lodash', spec: '^3.0.2' }
 *     isAvailable(spec, 'path/to/node_modules')
 */

export default async function isAvailable (spec: PackageSpec, modules: string) {
  const name = spec && spec.name
  if (!name) return false

  const packageJsonPath = path.join(modules, name, 'package.json')

  try {
    const content = await fs.readFile(packageJsonPath)
    const pkgJson = JSON.parse(content)
    return verify(spec, pkgJson)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return false
  }

  function verify (spec: PackageSpec, packageJson: Package) {
    return packageJson.name === spec.name &&
      ((spec.type !== 'range' && spec.type !== 'version') ||
      semver.satisfies(packageJson.version, spec.spec))
  }
}
