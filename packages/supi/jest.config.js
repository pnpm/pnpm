import config from '../../jest.config.js'

export default {
  ...config,
  testPathIgnorePatterns: [
    '<rootDir>/test/utils/distTags.ts',
    '<rootDir>/test/utils/index.ts',
    '<rootDir>/test/utils/testDefaults.ts',
  ],
}

