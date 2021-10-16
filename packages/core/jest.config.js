const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testPathIgnorePatterns: [
    '<rootDir>/test/utils/distTags.ts',
    '<rootDir>/test/utils/index.ts',
    '<rootDir>/test/utils/testDefaults.ts',
  ],
}

