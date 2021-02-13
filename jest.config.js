import path from 'path'

export default {
  preset: 'ts-jest/presets/default-esm',
  globals: {
    'ts-jest': {
      useESM: true,
    },
  },
  testMatch: ["**/test/**/*.[jt]s?(x)"],
  testEnvironment: 'node',
  collectCoverage: true,
  coveragePathIgnorePatterns: ['node_modules'],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(__dirname, 'jest.setup.js')],
};
