import path from 'path'
import baseConfig from './../config.js'

export default {
  ...baseConfig,
  // Many tests change the dist tags of packages.
  // Unfortunately, this means that if two such tests will run at the same time,
  // they may break each other.
  maxWorkers: 1,
  globalSetup: path.join(import.meta.dirname, 'globalSetup.js'),
  globalTeardown: path.join(import.meta.dirname, 'globalTeardown.js'),
}
