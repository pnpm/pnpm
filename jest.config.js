/**
 * Root Jest configuration for running all workspace tests with a single CLI call.
 * 
 * Usage:
 *   NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules" jest
 * 
 * Or to run specific packages:
 *   NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules" jest --selectProjects @pnpm/tarball-fetcher
 */
export default {
  // Use the projects configuration to discover all workspace packages with jest configs
  projects: [
    '<rootDir>/builder/*/package.json',
    '<rootDir>/cache/*/package.json',
    '<rootDir>/catalogs/*/package.json',
    '<rootDir>/cli/*/package.json',
    '<rootDir>/completion/*/package.json',
    '<rootDir>/config/*/package.json',
    '<rootDir>/crypto/*/package.json',
    '<rootDir>/dedupe/*/package.json',
    '<rootDir>/deps/*/package.json',
    '<rootDir>/env/*/package.json',
    '<rootDir>/exec/*/package.json',
    '<rootDir>/fetching/*/package.json',
    '<rootDir>/fs/*/package.json',
    '<rootDir>/hooks/*/package.json',
    '<rootDir>/lockfile/*/package.json',
    '<rootDir>/modules-mounter/*/package.json',
    '<rootDir>/network/*/package.json',
    '<rootDir>/object/*/package.json',
    '<rootDir>/packages/*/package.json',
    '<rootDir>/patching/*/package.json',
    '<rootDir>/pkg-manager/*/package.json',
    '<rootDir>/pkg-manifest/*/package.json',
    '<rootDir>/registry/*/package.json',
    '<rootDir>/releasing/*/package.json',
    '<rootDir>/resolving/*/package.json',
    '<rootDir>/reviewing/*/package.json',
    '<rootDir>/semver/*/package.json',
    '<rootDir>/store/*/package.json',
    '<rootDir>/text/*/package.json',
    '<rootDir>/testing/*/package.json',
    '<rootDir>/tools/*/package.json',
    '<rootDir>/worker/package.json',
    '<rootDir>/workspace/*/package.json',
    '<rootDir>/yaml/*/package.json',
    '<rootDir>/__utils__/test-ipc-server/package.json',
    // Note: pnpm/package.json is excluded as it has special test setup
  ],
}
