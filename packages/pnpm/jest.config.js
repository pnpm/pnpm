const config = require('../../jest.config.js')

module.exports = {
  ...config,
  testPathIgnorePatterns: [
    '<rootDir>/test/utils/distTags.ts',
    '<rootDir>/test/utils/execPnpm.ts',
    '<rootDir>/test/utils/index.ts',
    '<rootDir>/test/utils/localPkg.ts',
    '<rootDir>/test/utils/retryLoadJsonFile.ts',
    '<rootDir>/test/utils/testDefaults.ts',
  ],
}

