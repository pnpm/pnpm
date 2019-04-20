import { DependencyManifest } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'
import path = require('path')

const pkgJson = loadJsonFileSync<DependencyManifest>(path.resolve(__dirname, '../package.json'))
const packageManager = {
  name: pkgJson.name,
  version: pkgJson.version,
}
export default packageManager
