import { DependencyManifest } from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import path = require('path')

const pkgJson = loadJsonFile.sync<DependencyManifest>(path.resolve(__dirname, '../package.json'))
const packageManager = {
  name: pkgJson.name,
  // This may be a 3.0.0-beta.2
  version: pkgJson.version,
  // Never a prerelease version
  stableVersion: pkgJson.version.includes('-')
    ? pkgJson.version.substr(0, pkgJson.version.indexOf('-'))
    : pkgJson.version
}
export default packageManager
