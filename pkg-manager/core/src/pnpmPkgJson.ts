import path from 'path'
import { type PackageManifest } from '@pnpm/types'
import { loadJsonFileSync } from 'load-json-file'

let pnpmPkgJson!: PackageManifest
try {
  pnpmPkgJson = loadJsonFileSync<PackageManifest>(path.resolve(import.meta.dirname, '../package.json'))
} catch (err: any) { // eslint-disable-line
  pnpmPkgJson = {
    name: 'pnpm',
    version: '0.0.0',
  }
}
export { pnpmPkgJson }
