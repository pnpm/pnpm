import { DependencyManifest } from '@pnpm/types'
import loadJsonFile = require('load-json-file')
import path = require('path')

const pkgJson = loadJsonFile.sync<DependencyManifest>(path.resolve(__dirname, '../package.json'))
const packageManager = {
  name: pkgJson.name,
  version: pkgJson.version,
}
export default packageManager
