// Shared between setup.js (preinstall hook) and the test suite.
// Computes the npm package name of the matching @pnpm/exe platform child for a
// given host. Returns `@pnpm/<os>-<arch>`, where <os> is `macos` (darwin),
// `win` (win32), `linux` (glibc), or `linuxstatic` (musl). Pure — no I/O, no
// detect-libc call — so the musl branch is unit-testable without mocking.
export function exePlatformPkgName(platform, arch, libcFamily) {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  const osSegment =
    platform === 'darwin' ? 'macos'
    : platform === 'win32' ? 'win'
    : platform === 'linux' && libcFamily === 'musl' ? 'linuxstatic'
    : platform
  return `@pnpm/${osSegment}-${normalizedArch}`
}
