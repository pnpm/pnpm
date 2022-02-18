const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testPathIgnorePatterns: ["/fixtures/", "<rootDir>/test/utils/.+"],
}

