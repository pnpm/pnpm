const path = require('path')
const baseConfig = require('./config')

if (process.env.PNPM_REGISTRY_MOCK_PORT != null) {
  module.exports = {
    ...baseConfig,
    // Many tests change the dist tags of packages.
    // Unfortunately, this means that if two such tests will run at the same time,
    // they may break each other.
    maxWorkers: 1,
    globalSetup: path.join(__dirname, 'globalSetup.js'),
    globalTeardown: path.join(__dirname, 'globalTeardown.js'),
  }
} else {
  module.exports = baseConfig
}
