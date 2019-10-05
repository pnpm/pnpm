import { PackageManifest } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'
import path = require('path')

export default loadJsonFileSync<PackageManifest>(path.resolve(__dirname, '../package.json'))
