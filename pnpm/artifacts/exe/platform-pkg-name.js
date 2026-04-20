// Shared between setup.js (preinstall hook) and the test suite.
// Computes the npm package name of the matching @pnpm/exe platform child for a
// given host: `@pnpm/exe.<platform>-<arch>[-musl]`. Pure — no I/O, no detect-libc
// call — so the musl branch is unit-testable without mocking.
export function exePlatformPkgName(platform, arch, libcFamily) {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  const libcSuffix = platform === 'linux' && libcFamily === 'musl' ? '-musl' : ''
  return `@pnpm/exe.${platform}-${normalizedArch}${libcSuffix}`
}
