const path = require('path')

const config = {
  preset: "ts-jest",
  transform: {
    '^.+\\.tsx?$': ['ts-jest', {
      // For most projects, the tsconfig.json and test/tsconfig.json are almost
      // exactly the same. But it's more correct to point to test/tsconfig.json
      // to prevent surprises in the future.
      tsconfig: 'test/tsconfig.json'
    }]
  },
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  collectCoverage: true,
  coveragePathIgnorePatterns: ["/node_modules/"],
  testPathIgnorePatterns: ["/fixtures/", "/__fixtures__/", "<rootDir>/test/utils/.+"],
  modulePathIgnorePatterns: ['\/__fixtures__\/.*'],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(__dirname, "setupFilesAfterEnv.js")],
  maxWorkers: "50%",
}

if (process.env.PNPM_SCRIPT_SRC_DIR) {
  const pathAsArr = process.env.PNPM_SCRIPT_SRC_DIR.split(path.sep)
  const packageName = pathAsArr[pathAsArr.length - 1]
  config.cacheDirectory = path.join(__dirname, ".jest-cache", packageName)
}

module.exports = config
