// Shared between setup.js (preinstall hook) and the test suite.
// exePlatformPkgName computes the npm package name of the matching @pnpm/exe
// platform child for a given host. Returns `@pnpm/<os>-<arch>`, where <os> is
// `macos` (darwin), `win` (win32), `linux` (glibc), or `linuxstatic` (musl).
// Pure — no I/O, no detect-libc call — so the musl branch is unit-testable
// without mocking.
export function exePlatformPkgName(platform, arch, libcFamily) {
  const normalizedArch = platform === 'win32' && arch === 'ia32' ? 'x86' : arch
  return `@pnpm/${legacyOsSegment(platform, libcFamily)}-${normalizedArch}`
}

function legacyOsSegment(platform, libcFamily) {
  switch (platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'win'
    case 'linux': return libcFamily === 'musl' ? 'linuxstatic' : 'linux'
    default: return platform
  }
}

// The error setup.js prints when the host's platform package is not
// installed. @pnpm/exe deliberately ships no binary for two hosts, where
// Node.js SEA injection produces one that segfaults at startup.
export function missingPlatformPkgMessage(platform, arch, libcFamily) {
  if (platform === 'darwin' && arch === 'x64') {
    return '@pnpm/exe does not ship a working binary for Intel macOS (darwin-x64) due to an upstream Node.js SEA bug.\n' +
      'See https://github.com/pnpm/pnpm/issues/11423 and https://github.com/nodejs/node/issues/62893.\n' +
      'Workaround: install pnpm via `npm install -g pnpm` (uses your system Node.js, no SEA), or use pnpm 10.x.'
  }
  if (platform === 'linux' && arch === 'arm64' && libcFamily === 'musl') {
    return '@pnpm/exe does not ship a working binary for arm64 musl Linux (such as Alpine on ARM).\n' +
      'See https://github.com/pnpm/pnpm/issues/10443.\n' +
      'Workaround: install pnpm via `npm install -g pnpm` (uses your system Node.js, no SEA), or use pnpm 12, which has a native binary for this platform.'
  }
  const pkgName = exePlatformPkgName(platform, arch, libcFamily)
  return `Could not find platform package "${pkgName}" — @pnpm/exe does not ship a binary for ${platform}-${arch}.`
}
