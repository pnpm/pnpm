const path = require("path")

const config = {
  preset: "ts-jest",
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  collectCoverage: true,
  coveragePathIgnorePatterns: ["/node_modules/"],
  testPathIgnorePatterns: ["/fixtures/", "/__fixtures__/", "<rootDir>/test/utils/.+"],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(__dirname, "jest.setup.js")],
  maxWorkers: "50%",
}

if (process.env.PNPM_SCRIPT_SRC_DIR) {
  const pathAsArr = process.env.PNPM_SCRIPT_SRC_DIR.split(path.sep)
  const packageName = pathAsArr[pathAsArr.length - 1]
  config.cacheDirectory = path.join(__dirname, ".jest-cache", packageName)
}

module.exports = config
