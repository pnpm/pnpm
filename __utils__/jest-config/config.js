import path from 'path'

const config = {
  resolver: path.join(import.meta.dirname, 'node_modules/ts-jest-resolver'),
  extensionsToTreatAsEsm: ['.ts'],
  transform: {
    '^.+\\.tsx?$': path.join(import.meta.dirname, 'jest.transform.js'),
  },
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  // Allow Just coverage to be configured through an environment variable. This
  // can be useful for pnpm developers working locally that only want to run one
  // test suite. Jest coverage collection setup scripts also contain a
  // "debugger;" keyword usage that causes an interactive debugger to pause,
  // which can be annoying.
  collectCoverage: Boolean(process.env.PNPM_JEST_COLLECT_COVERAGE ?? true),
  coveragePathIgnorePatterns: ["/node_modules/"],
  testPathIgnorePatterns: ["/fixtures/", "/__fixtures__/", "<rootDir>/test/utils/.+"],
  modulePathIgnorePatterns: ['\/__fixtures__\/.*'],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(import.meta.dirname, "setupFilesAfterEnv.js")],
  maxWorkers: "50%",
}

if (process.env.PNPM_SCRIPT_SRC_DIR) {
  const pathAsArr = process.env.PNPM_SCRIPT_SRC_DIR.split(path.sep)
  const packageName = pathAsArr[pathAsArr.length - 1]
  config.cacheDirectory = path.join(import.meta.dirname, ".jest-cache", packageName)
}

// We are running test script from pnpm command, this seems to confuse tests
// Clean up env from pnpm variables so that nested pnpm runs won't get affected on config read
for (const key of Object.keys(process.env)) {
  if (/^p?npm_(config|package|lifecycle|node|command|execpath)(_|$)/ui.test(key)) {
    delete process.env[key]
  }
}

export default config
