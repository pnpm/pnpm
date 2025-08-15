import { fileURLToPath } from 'url'
import path from 'path'
import baseConfig from './../config.js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

export default {
  ...baseConfig,
  // Many tests change the dist tags of packages.
  // Unfortunately, this means that if two such tests will run at the same time,
  // they may break each other.
  maxWorkers: 1,
  globalSetup: path.join(__dirname, 'globalSetup.js'),
  globalTeardown: path.join(__dirname, 'globalTeardown.js'),
}
