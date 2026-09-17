import { spawnSync } from 'node:child_process'
import console from 'node:console'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

// `cargo fmt` reads the edition from each crate's manifest. A bare `rustfmt`
// has to be told, and every crate here inherits the one the workspace manifest
// sets.
const EDITION = '2024'

/**
 * The staged files to format, and the ones held back.
 *
 * A file that also has unstaged changes is left alone: formatting the working
 * tree and staging the result would commit the part of that file the author
 * deliberately kept out of this commit.
 */
export function partitionStaged (staged, unstaged) {
  const alsoUnstaged = new Set(unstaged)
  return {
    formattable: staged.filter(file => !alsoUnstaged.has(file)),
    withheld: staged.filter(file => alsoUnstaged.has(file)),
  }
}

/**
 * Formats the Rust files being committed and stages the result.
 *
 * Formatting is the one Rust check that needs no compile, so it is the one
 * that can afford to run per commit. Clippy and dylint stay in pre-push, where
 * their cost is paid once per push rather than once per commit.
 */
export function formatStagedRust (repo, { format = pinnedRustfmt } = {}) {
  const staged = gitPaths(repo, ['diff', '--cached', '--name-only', '--diff-filter=ACM', '-z', '--', '*.rs'])
  if (staged.length === 0) return 0

  const unstaged = gitPaths(repo, ['diff', '--name-only', '-z', '--', '*.rs'])
  const { formattable, withheld } = partitionStaged(staged, unstaged)
  if (withheld.length > 0) {
    console.error(`pre-commit: not formatting these files, each has unstaged changes too:\n${indent(withheld)}`)
  }
  if (formattable.length === 0) return 0

  // Absolute paths: a repository-relative path that begins with `-` would
  // otherwise reach rustfmt as an option.
  const status = format(formattable.map(file => path.join(repo, file)))
  if (status !== 0) return status

  const reformatted = formattable.filter(file => git(repo, ['diff', '--quiet', '--', file]).status !== 0)
  if (reformatted.length === 0) return 0

  checkedGit(repo, ['add', '--', ...reformatted])
  console.log(`pre-commit: formatted and staged:\n${indent(reformatted)}`)
  return 0
}

function pinnedRustfmt (files) {
  const script = fileURLToPath(new URL('./rustfmt.mjs', import.meta.url))
  const result = spawnSync(process.execPath, [script, '--rustfmt', '--edition', EDITION, ...files], { stdio: 'inherit' })
  if (result.error != null) throw result.error
  return result.status ?? 1
}

function gitPaths (repo, args) {
  return checkedGit(repo, args).stdout.split('\0').filter(entry => entry !== '')
}

function git (repo, args) {
  const result = spawnSync('git', args, { encoding: 'utf8', cwd: repo })
  if (result.error != null) throw result.error
  return result
}

function checkedGit (repo, args) {
  const result = git(repo, args)
  if (result.status !== 0) throw new Error(`git ${args.join(' ')} failed:\n${result.stderr}`)
  return result
}

function indent (files) {
  return files.map(file => `  ${file}`).join('\n')
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    const repo = spawnSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).stdout.trim()
    process.exitCode = formatStagedRust(repo)
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
