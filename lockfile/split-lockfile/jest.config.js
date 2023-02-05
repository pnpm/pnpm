const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testMatch: [...config.testMatch, "!**/test/utils.ts"]
}
