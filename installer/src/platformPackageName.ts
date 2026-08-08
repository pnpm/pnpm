export interface Target {
  /** Major version of the pnpm release being installed. */
  major: number
  /** `process.platform` of the host. */
  platform: string
  /** `process.arch` of the host. */
  arch: string
  /** libc family of the host, as reported by `detect-libc`. */
  libcFamily: string | null
}

/**
 * Name of the npm package that carries the pnpm executable for `target`.
 *
 * pnpm ships the binary in a per-host package that `@pnpm/exe` lists as an
 * optional dependency. The naming scheme changed with the Rust rewrite: v12
 * publishes `@pnpm/exe.<process.platform>-<arch>[-musl]`, while v11 and older
 * publish `@pnpm/<macos|win|linux|linuxstatic>-<arch>`.
 *
 * @throws if pnpm publishes no binary for the host — either because the
 * architecture was never supported, or because of the v11-only Intel macOS gap.
 */
export function platformPackageName ({ major, platform, arch, libcFamily }: Target): string {
  if (arch !== 'x64' && arch !== 'arm64') {
    throw new Error('Sorry! pnpm currently only provides pre-built binaries for x86_64/arm64 architectures.')
  }
  if (platform !== 'darwin' && platform !== 'linux' && platform !== 'win32') {
    throw new Error(`Sorry! pnpm does not provide a pre-built binary for ${platform}.`)
  }
  if (platform === 'darwin' && arch === 'x64' && major === 11) {
    throw new Error(`pnpm v11 does not provide a working binary for Intel macOS (darwin-x64) due to an upstream Node.js SEA bug.

Install pnpm a different way instead:
  npx get-pnpm 12        # pnpm v12 ships an Intel macOS binary
  npm install -g pnpm    # uses your system Node.js
  brew install pnpm      # via Homebrew

More context: https://github.com/pnpm/pnpm/issues/11423`)
  }
  const isMusl = platform === 'linux' && libcFamily === 'musl'
  if (major >= 12) {
    return `@pnpm/exe.${platform}-${arch}${isMusl ? '-musl' : ''}`
  }
  return `@pnpm/${legacyOsSegment(platform, isMusl)}-${arch}`
}

function legacyOsSegment (platform: string, isMusl: boolean): string {
  switch (platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'win'
    default: return isMusl ? 'linuxstatic' : 'linux'
  }
}
