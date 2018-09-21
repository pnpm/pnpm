import { PackageJson } from '@pnpm/types'
import { sync as loadJsonFileSync } from 'load-json-file'
import path = require('path')

export default loadJsonFileSync<PackageJson>(path.resolve(__dirname, '../package.json'))
