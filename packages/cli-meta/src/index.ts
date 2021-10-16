import path from 'path'
import { DependencyManifest } from '@pnpm/types'
import loadJsonFile from 'load-json-file'

const defaultManifest = {
  name: process.env.npm_package_name != null && process.env.npm_package_name !== ''
    ? process.env.npm_package_name
    : 'pnpm',
  version: process.env.npm_package_version != null && process.env.npm_package_version !== ''
    ? process.env.npm_package_version
    : '0.0.0',
}
let pkgJson
if (require.main == null) {
  pkgJson = defaultManifest
} else {
  try {
    pkgJson = {
      ...defaultManifest,
      ...loadJsonFile.sync<DependencyManifest>(
        path.join(path.dirname(require.main.filename), '../package.json')
      ),
    }
  } catch (err: any) { // eslint-disable-line
    pkgJson = defaultManifest
  }
}

const packageManager = {
  name: pkgJson.name,
  // Never a prerelease version
  stableVersion: pkgJson.version.includes('-')
    ? pkgJson.version.substr(0, pkgJson.version.indexOf('-'))
    : pkgJson.version,
  // This may be a 3.0.0-beta.2
  version: pkgJson.version,
}
export default packageManager
