import { DependencyManifest } from '@pnpm/types'
import path = require('path')
import loadJsonFile = require('load-json-file')

const defaultManifest = {
  name: 'unknown',
  version: '0.0.0',
}
let pkgJson
try {
  pkgJson = {
    ...defaultManifest,
    ...loadJsonFile.sync<DependencyManifest>(
      path.join(path.dirname(require.main!.filename), '../package.json')
    ),
  }
} catch (err) {
  pkgJson = defaultManifest
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
