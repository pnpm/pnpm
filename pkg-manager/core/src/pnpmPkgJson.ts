import path from 'path'
import { type PackageManifest } from '@pnpm/types'
import { syncJSON } from '@pnpm/file-reader'

let pnpmPkgJson!: PackageManifest
try {
  pnpmPkgJson = syncJSON<PackageManifest>(path.resolve(__dirname, '../package.json'))
} catch {
  pnpmPkgJson = {
    name: 'pnpm',
    version: '0.0.0',
  }
}
export { pnpmPkgJson }
