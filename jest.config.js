const path = require("path")

const config = {
  projects: [
    'cli/*',
    'config/*',
    'dedupe/*',
    'deps/*',
    'env/*',
    'exec/*',
    'fetching/*',
    'fs/*',
    'hooks/*',
    'lockfile/*',
    'network/*',
    'modules-mounter/*',
    'packages/*',
    'pkg-manager/*',
    'pkg-manifest/*',
    'patching/*',
    'pnpm',
    'releasing/*',
    'resolving/*',
    'reviewing/*',
    'store/*',
    'text/*',
    'workspace/*',
  ],
  preset: "ts-jest",
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  collectCoverage: true,
  coveragePathIgnorePatterns: ["/node_modules/"],
  testPathIgnorePatterns: ["/fixtures/", "/__fixtures__/", "<rootDir>/test/utils/.+"],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(__dirname, "jest.setup.js")],
  // Many tests change the dist tags of packages.
  // Unfortunately, this means that if two such tests will run at the same time,
  // they may break each other.
  // maxWorkers: '100%',
}

if (process.env.PNPM_SCRIPT_SRC_DIR) {
  const pathAsArr = process.env.PNPM_SCRIPT_SRC_DIR.split(path.sep)
  const packageName = pathAsArr[pathAsArr.length - 1]
  config.cacheDirectory = path.join(__dirname, ".jest-cache", packageName)
}

module.exports = config
