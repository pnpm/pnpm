import path from 'path'
import { fileURLToPath } from 'url'

const dirname = path.dirname(fileURLToPath(import.meta.url))

export default {
  preset: 'ts-jest/presets/default-esm',
  globals: {
    'ts-jest': {
      useESM: true,
    },
  },
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: 'node',
  collectCoverage: true,
  coveragePathIgnorePatterns: ['node_modules'],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(dirname, 'jest.setup.js')],
};
