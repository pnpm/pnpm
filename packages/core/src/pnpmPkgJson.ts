import path from 'path'
import { PackageManifest } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'

let pkgJson!: PackageManifest
try {
  pkgJson = loadJsonFileSync<PackageManifest>(path.resolve(__dirname, '../package.json'))
} catch (err: any) { // eslint-disable-line
  pkgJson = {
    name: 'pnpm',
    version: '0.0.0',
  }
}
export default pkgJson
