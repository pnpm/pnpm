import fs from 'fs'
import path from 'path'

// Reflink platform package names needed for a build target, matching the package
// names used by @reflink/reflink's binding.js require() fallback.
// Target format examples: 'linux-x64', 'linuxstatic-arm64', 'macos-arm64', 'win-x64'.
export function getReflinkKeepPackages (target: string): string[] {
  if (target.startsWith('macos-')) {
    const arch = target.slice('macos-'.length)
    return [`@reflink/reflink-darwin-${arch}`]
  }
  if (target.startsWith('win-')) {
    const arch = target.slice('win-'.length)
    return [`@reflink/reflink-win32-${arch}-msvc`]
  }
  if (target.startsWith('linux')) {
    // Keep both glibc and musl variants — the correct one is picked at runtime.
    const arch = target.includes('arm64') ? 'arm64' : 'x64'
    return [
      `@reflink/reflink-linux-${arch}-gnu`,
      `@reflink/reflink-linux-${arch}-musl`,
    ]
  }
  return []
}

/**
 * Remove reflink platform packages from dist/node_modules/@reflink/.
 * If keepPackages is provided, only those packages are kept; everything else
 * under @reflink/ except the main @reflink/reflink package is removed.
 * If keepPackages is empty/undefined, ALL platform packages are removed
 * (the main @reflink/reflink package is kept).
 */
export function stripReflinkPackages (distDir: string, keepPackages?: string[]): void {
  const reflinkDir = path.join(distDir, 'node_modules', '@reflink')
  if (!fs.existsSync(reflinkDir)) return

  for (const entry of fs.readdirSync(reflinkDir)) {
    if (entry === 'reflink') continue // keep the main package
    const pkgName = `@reflink/${entry}`
    if (!keepPackages || !keepPackages.includes(pkgName)) {
      fs.rmSync(path.join(reflinkDir, entry), { recursive: true })
    }
  }
}
