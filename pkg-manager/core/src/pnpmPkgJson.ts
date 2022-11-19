import path from 'path'
import { PackageManifest } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'

let pnpmPkgJson!: PackageManifest
try {
  pnpmPkgJson = loadJsonFileSync<PackageManifest>(path.resolve(__dirname, '../package.json'))
} catch (err: any) { // eslint-disable-line
  pnpmPkgJson = {
    name: 'pnpm',
    version: '0.0.0',
  }
}
export { pnpmPkgJson }
