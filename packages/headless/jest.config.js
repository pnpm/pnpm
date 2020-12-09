const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testTimeout: 240000,
  testMatch: ["**/test/index.ts"],
}
