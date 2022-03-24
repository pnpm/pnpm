import { spawnSync } from 'child_process'

export function getCurrentBranchName () {
  const { stdout } = spawnSync('git', ['symbolic-ref', '--short', 'HEAD'], { encoding: 'utf8' })
  return String(stdout).trim()
}