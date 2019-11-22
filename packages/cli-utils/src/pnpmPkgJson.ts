import { DependencyManifest } from '@pnpm/types'
import loadJsonFile = require('load-json-file')

const pkgJson = loadJsonFile.sync<DependencyManifest>(require.resolve('pnpm/package.json', { paths: [__dirname] }))
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
