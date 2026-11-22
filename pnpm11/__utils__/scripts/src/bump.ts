// Applies the pending release plan, then runs the meta-updater to sync the
// manifests it derives.

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

function main (): void {
  const repoRoot = findRepoRoot(import.meta.dirname)
  // The release PR branch is dirty here (refreshed trust roots, synthesized
  // changesets), so skip the clean-tree check.
  execFileSync('pnpm', ['version', '-r', '--no-git-checks'], { cwd: repoRoot, stdio: 'inherit' })
  execFileSync('pnpm', ['update-manifests'], { cwd: repoRoot, stdio: 'inherit' })
}

export function findRepoRoot (startDir: string): string {
  let dir = startDir
  while (!fs.existsSync(path.join(dir, '.changeset'))) {
    const parent = path.dirname(dir)
    if (parent === dir) {
      throw new Error(`No .changeset directory found above ${startDir}`)
    }
    dir = parent
  }
  return dir
}

function isDirectInvocation (): boolean {
  if (process.argv[1] === undefined) return false
  try {
    return import.meta.url === url.pathToFileURL(fs.realpathSync(process.argv[1])).href
  } catch {
    return false
  }
}

if (isDirectInvocation()) {
  main()
}
