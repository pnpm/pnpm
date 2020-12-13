const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testPathIgnorePatterns: [
    '<rootDir>/test/utils.ts',
  ],
}
