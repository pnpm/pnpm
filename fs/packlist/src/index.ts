import fs from 'fs/promises'
import path from 'path'
import util from 'util'
import npmPacklist from 'npm-packlist'

export async function packlist (pkgDir: string, opts?: {
  manifest?: Record<string, unknown>
}): Promise<string[]> {
  const pkg = opts?.manifest ?? await readPackageJson(pkgDir)
  const tree = {
    path: pkgDir,
    package: normalizePackage(pkg),
    isProjectRoot: true,
    edgesOut: new Map(),
  }
  const files = await npmPacklist(tree)
  return files.map((file) => file.replace(/^\.[/\\]/, ''))
}

async function readPackageJson (dir: string): Promise<Record<string, unknown>> {
  try {
    return JSON.parse(await fs.readFile(path.join(dir, 'package.json'), 'utf8'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return {}
    }
    throw err
  }
}

function stripDotSlash (p: string): string {
  return p.replace(/^\.[/\\]/, '')
}

function normalizePackage (pkg: Record<string, unknown>): Record<string, unknown> {
  const normalized = { ...pkg }
  if (typeof normalized.main === 'string') {
    normalized.main = stripDotSlash(normalized.main)
  }
  if (typeof normalized.browser === 'string') {
    normalized.browser = stripDotSlash(normalized.browser)
  }
  if (typeof normalized.bin === 'string') {
    normalized.bin = stripDotSlash(normalized.bin)
  } else if (normalized.bin != null && typeof normalized.bin === 'object') {
    const bin: Record<string, string> = {}
    for (const [key, value] of Object.entries(normalized.bin as Record<string, string>)) {
      bin[key] = stripDotSlash(value)
    }
    normalized.bin = bin
  }
  return normalized
}
