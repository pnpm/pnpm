export interface Target {
  /** Major version of the pnpm release being installed. */
  major: number
  /** `process.platform` of the host. */
  platform: string
  /** `process.arch` of the host. */
  arch: string
  /** Whether the host's libc is musl rather than glibc. */
  musl: boolean
}

const V12_ONLY_TARGETS = new Set([
  'android-arm64',
  'android-x64',
  'freebsd-x64',
  'linux-ppc64',
  'linux-riscv64',
  'linux-s390x',
])

/**
 * Name of the npm package that carries the pnpm executable for `target`.
 *
 * pnpm ships the binary in a per-host package that `@pnpm/exe` lists as an
 * optional dependency. The naming scheme changed with the Rust rewrite: v12
 * publishes `@pnpm/exe.<process.platform>-<arch>[-musl]`, while v11 and older
 * publish `@pnpm/<macos|win|linux|linuxstatic>-<arch>`.
 *
 * @throws if pnpm publishes no binary for the host — because the host is not
 * built for at all, because the binary arrived in v12 and an older major was
 * asked for, or because of the v11-only Intel macOS gap.
 */
export function platformPackageName ({ major, platform, arch, musl }: Target): string {
  const target = `${platform}-${arch}`
  if (V12_ONLY_TARGETS.has(target)) {
    if (major >= 12) return `@pnpm/exe.${target}`
    throw new Error(`pnpm v${major} does not provide a pre-built binary for ${target}. pnpm 12 does:

  npx get-pnpm 12`)
  }
  assertBinaryIsPublished({ major, platform, arch })
  const linuxMusl = platform === 'linux' && musl
  if (major >= 12) {
    return `@pnpm/exe.${platform}-${arch}${linuxMusl ? '-musl' : ''}`
  }
  return `@pnpm/${legacyOsSegment(platform, linuxMusl)}-${arch}`
}

/** Throws for the hosts outside v12's own targets that a release leaves out. */
function assertBinaryIsPublished ({ major, platform, arch }: Omit<Target, 'musl'>): void {
  const target = `${platform}-${arch}`
  if ((arch !== 'x64' && arch !== 'arm64') ||
    (platform !== 'darwin' && platform !== 'linux' && platform !== 'win32')) {
    throw new Error(`Sorry! pnpm does not provide a pre-built binary for ${target}.`)
  }
  if (platform === 'darwin' && arch === 'x64' && major === 11) {
    throw new Error(`pnpm v11 does not provide a working binary for Intel macOS (darwin-x64) due to an upstream Node.js SEA bug.

Install pnpm a different way instead:
  npx get-pnpm 12        # pnpm v12 ships an Intel macOS binary
  npm install -g pnpm    # uses your system Node.js
  brew install pnpm      # via Homebrew

More context: https://github.com/pnpm/pnpm/issues/11423`)
  }
}

function legacyOsSegment (platform: string, isMusl: boolean): string {
  switch (platform) {
    case 'darwin': return 'macos'
    case 'win32': return 'win'
    default: return isMusl ? 'linuxstatic' : 'linux'
  }
}

/**
 * Whether this host's libc is musl rather than glibc.
 *
 * Probed the way pnpm's own wrappers do it rather than through a dependency:
 * glibc builds report a `glibcVersionRuntime`, musl builds leave it unset.
 * Every dependency is one more package `npx` fetches before pnpm exists.
 */
export function isMusl (): boolean {
  if (process.platform !== 'linux') return false
  try {
    const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined
    return report?.header != null && !report.header.glibcVersionRuntime
  } catch {
    return false
  }
}
