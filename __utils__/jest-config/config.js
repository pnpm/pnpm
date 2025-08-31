import path from 'path'

const config = {
  preset: "ts-jest/presets/default-esm",
  resolver: path.join(import.meta.dirname, 'node_modules/ts-jest-resolver'),
  globals: {
    'ts-jest': {
      useESM: true,
    },
  },
  extensionsToTreatAsEsm: ['.ts'],
  transform: {
    '^.+\\.tsx?$': ['ts-jest', {
      // For most projects, the tsconfig.json and test/tsconfig.json are almost
      // exactly the same. But it's more correct to point to test/tsconfig.json
      // to prevent surprises in the future.
      tsconfig: 'test/tsconfig.json'
    }],
  },
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  collectCoverage: true,
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
