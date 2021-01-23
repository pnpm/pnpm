import { PackageManifest } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'
import path = require('path')

let pkgJson!: PackageManifest
try {
  pkgJson = loadJsonFileSync<PackageManifest>(path.resolve(__dirname, '../package.json'))
} catch (err) {
  pkgJson = {
    name: 'pnpm',
    version: '0.0.0',
  }
}
export default pkgJson
