import path from 'path'

const config = {
  resolver: path.join(import.meta.dirname, 'node_modules/ts-jest-resolver'),
  extensionsToTreatAsEsm: ['.ts'],
  transform: {
    '^.+\\.tsx?$': path.join(import.meta.dirname, 'jest.transform.js'),
  },
  testMatch: ["**/test/**/*.[jt]s?(x)", "**/src/**/*.test.ts"],
  testEnvironment: "node",
  collectCoverage: true,
  coveragePathIgnorePatterns: ["/node_modules/"],
  testPathIgnorePatterns: ["/fixtures/", "/__fixtures__/", "/test/(.+/)?utils/"],
  modulePathIgnorePatterns: ['\/__fixtures__\/.*'],
  testTimeout: 4 * 60 * 1000, // 4 minutes
  setupFilesAfterEnv: [path.join(import.meta.dirname, "setupFilesAfterEnv.js")],
  maxWorkers: "50%",
}

if (process.env.PNPM_SCRIPT_SRC_DIR) {
  config.cacheDirectory = getCacheDirectory(process.env.PNPM_SCRIPT_SRC_DIR)
}

// We are running test script from pnpm command, this seems to confuse tests
// Clean up env from pnpm variables so that nested pnpm runs won't get affected on config read
for (const key of Object.keys(process.env)) {
  if (/^p?npm_(config|package|lifecycle|node|command|execpath)(_|$)/ui.test(key)) {
    delete process.env[key]
  }
}

export default config

export function getCacheDirectory (projectDir) {
  const workspaceDir = path.join(import.meta.dirname, '../../..')
  return path.join(import.meta.dirname, '.jest-cache', path.relative(workspaceDir, projectDir))
}
