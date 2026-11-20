const OS_BY_TOKEN = new Map([
  ['aix', 'aix'],
  ['android', 'android'],
  ['darwin', 'darwin'],
  ['macos', 'darwin'],
  ['osx', 'darwin'],
  ['freebsd', 'freebsd'],
  ['linux', 'linux'],
  ['netbsd', 'netbsd'],
  ['openbsd', 'openbsd'],
  ['openharmony', 'openharmony'],
  ['sunos', 'sunos'],
  ['win32', 'win32'],
  ['windows', 'win32'],
])

const CPU_BY_TOKEN = new Map([
  ['arm', 'arm'],
  ['armv6', 'arm'],
  ['armv7', 'arm'],
  ['arm64', 'arm64'],
  ['aarch64', 'arm64'],
  ['ia32', 'ia32'],
  ['loong64', 'loong64'],
  ['mips64el', 'mips64el'],
  ['ppc64', 'ppc64'],
  ['ppc64le', 'ppc64'],
  ['riscv64', 'riscv64'],
  ['s390x', 's390x'],
  ['x64', 'x64'],
  ['amd64', 'x64'],
  ['wasm32', 'wasm32'],
])

const LIBC_BY_TOKEN = new Map([
  ['glibc', 'glibc'],
  ['gnu', 'glibc'],
  ['gnueabihf', 'glibc'],
  ['musl', 'musl'],
  ['musleabihf', 'musl'],
])

export interface PlatformInferredFromPackageName {
  os?: string[]
  cpu?: string[]
  libc?: string[]
}

/**
 * Infers the supported platforms of a package from the tokens of its name,
 * e.g. `@nx/nx-win32-arm64-msvc` → `{ os: ['win32'], cpu: ['arm64'] }`.
 * Platform-specific binary packages follow this naming convention, which is
 * the only platform signal left when their os/cpu/libc manifest fields are
 * absent. Returns null when no platform token is recognized in the name.
 */
export function inferPlatformFromPackageName (name: string): PlatformInferredFromPackageName | null {
  const nameWithoutScope = name.includes('/') ? name.slice(name.indexOf('/') + 1) : name
  const tokens = nameWithoutScope.toLowerCase().split(/[-_.]/)
  const os = pickTokenValues(tokens, OS_BY_TOKEN)
  const cpu = pickTokenValues(tokens, CPU_BY_TOKEN)
  const libc = pickTokenValues(tokens, LIBC_BY_TOKEN)
  if (os == null && cpu == null && libc == null) return null
  return {
    ...(os != null ? { os } : {}),
    ...(cpu != null ? { cpu } : {}),
    ...(libc != null ? { libc } : {}),
  }
}

function pickTokenValues (tokens: string[], valueByToken: Map<string, string>): string[] | undefined {
  const values = new Set<string>()
  for (const token of tokens) {
    const value = valueByToken.get(token)
    if (value != null) {
      values.add(value)
    }
  }
  return values.size > 0 ? Array.from(values) : undefined
}
