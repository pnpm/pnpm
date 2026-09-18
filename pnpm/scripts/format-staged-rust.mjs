import { spawnSync } from 'node:child_process'
import console from 'node:console'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

// `cargo fmt` reads the edition from each crate's manifest. A bare `rustfmt`
// has to be told, and every crate here inherits the one the workspace manifest
// sets.
const EDITION = '2024'

/**
 * Formats the Rust files being committed and stages the result.
 *
 * Formatting is the one Rust check that needs no compile, so it is the one
 * that can afford to run per commit. Clippy and dylint stay in pre-push, where
 * their cost is paid once per push rather than once per commit.
 */
export function formatStagedRust (repo, { format = pinnedRustfmt } = {}) {
  // `R` is load-bearing: git reports a renamed file that was edited as one
  // rename rather than an addition, so without it the destination of a
  // `git mv` is committed unformatted.
  const staged = gitPaths(repo, ['diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z', '--', '*.rs'])
  if (staged.length === 0) return 0

  const root = fs.realpathSync(repo)
  const unstaged = gitPaths(repo, ['diff', '--name-only', '-z', '--', '*.rs'])
  const { formattable, withheld, foreign } = partitionStaged(root, staged, unstaged)
  if (withheld.length > 0) {
    console.error(`pre-commit: not formatting these files, each has unstaged changes too:\n${indent(withheld)}`)
  }
  if (foreign.length > 0) {
    console.error(`pre-commit: not formatting these paths, none is a regular file in the checkout:\n${indent(foreign)}`)
  }
  if (formattable.length === 0) return 0

  const status = format(formattable.map(file => path.join(root, file)))
  if (status !== 0) return status

  const reformatted = formattable.filter(file => git(repo, ['diff', '--quiet', '--', literal(file)]).status !== 0)
  if (reformatted.length === 0) return 0

  checkedGit(repo, ['add', '--', ...reformatted.map(literal)])
  console.log(`pre-commit: formatted and staged:\n${indent(reformatted)}`)
  return 0
}

/**
 * The staged files to format, and the ones to leave alone.
 *
 * A file that also has unstaged changes is held back: formatting the working
 * tree and staging the result would commit the part of that file the author
 * deliberately kept out of this commit. A path that does not lead to a regular
 * file inside the checkout is not a source at all — rustfmt writes through a
 * symlink, so such a path would have it rewrite a file the commit never
 * touches, and the repository would show nothing changed.
 */
function partitionStaged (root, staged, unstaged) {
  const alsoUnstaged = new Set(unstaged)
  const formattable = []
  const withheld = []
  const foreign = []
  for (const file of staged) {
    if (!isCheckedOutSource(root, file)) foreign.push(file)
    else if (alsoUnstaged.has(file)) withheld.push(file)
    else formattable.push(file)
  }
  return { formattable, withheld, foreign }
}

/**
 * Whether a staged path leads to a regular file inside the checkout.
 *
 * Git will not index a path beyond a symbolic link, and reports one whose
 * directory became a link afterwards as deleted from the working tree. This
 * states the invariant those two behaviors happen to give rather than leaving
 * it to them, so `root` must already be a resolved path.
 */
export function isCheckedOutSource (root, file) {
  const absolute = path.join(root, file)
  try {
    if (!fs.lstatSync(absolute).isFile()) return false
    return fs.realpathSync(absolute).startsWith(root + path.sep)
  } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

function pinnedRustfmt (files) {
  const script = fileURLToPath(new URL('./rustfmt.mjs', import.meta.url))
  const result = spawnSync(process.execPath, [script, '--rustfmt', '--edition', EDITION, ...files], { stdio: 'inherit' })
  if (result.error != null) throw result.error
  return result.status ?? 1
}

// git reads a pathspec as a glob, so `star*.rs` would name `starX.rs` as well.
function literal (file) {
  return `:(literal)${file}`
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
