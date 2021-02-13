import config from '../../jest.config.js'

export default {
  ...config,
  testPathIgnorePatterns: [
    '<rootDir>/test/utils.ts',
  ],
}
