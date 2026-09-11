import fs from 'fs'
import path from 'path'

const ownDir = import.meta.dirname
const placeholder = 'This file intentionally left blank'

// pnpm is a placeholder — replaced with a hardlink to the native binary by setup.js
for (const name of ['pnpm']) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, placeholder, 'utf8')
}

// pn, pnpx, and pnx — write the real shell scripts and Windows wrappers.
//
// The Unix scripts hand over to the pnpm installed alongside them, found
// relative to the script: a `PATH` lookup would run whatever other pnpm comes
// first there, and would find nothing at all when the directory holding these
// bins is not on `PATH`. On Windows setup.js hardlinks the binary onto
// pn.exe/pnpx.exe/pnx.exe and points `bin` at them, so the .cmd and .ps1
// wrappers below only run when setup.js did not — where there is no sibling
// binary to resolve and `PATH` is all they have.
for (const [name, subcommand] of [['pn', ''], ['pnpx', ' dlx'], ['pnx', ' dlx']]) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, unixScript(name, subcommand), { mode: 0o755 })
  fs.writeFileSync(path.join(ownDir, name + '.cmd'), `@echo off\npnpm${subcommand} %*\n`)
  fs.writeFileSync(path.join(ownDir, name + '.ps1'), `pnpm${subcommand} @args\n`)
}

function unixScript (name, subcommand) {
  return `#!/bin/sh
# $0 is whatever shim or symlink \`${name}\` was launched through, so walk to the
# file itself before looking beside it. The hop cap matches the kernel's ELOOP
# limit, so a cycle cannot hang the script. Directories come from \`\${self%/*}\`
# and \`readlink\` runs through \`command -p\`, so the caller's PATH decides nothing here.
self=$0
# \`\${self%/*}\` needs a slash to strip. A bare name came from a PATH lookup and
# stands for a file in the current directory.
case $self in
  */*) ;;
  *) self=./$self ;;
esac
hops=0
while [ -L "$self" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops + 1))
  link=$(command -p readlink "$self")
  case $link in
    /*) self=$link ;;
    *) self=\${self%/*}/$link ;;
  esac
done

# The walk has to end at a regular file. Running out of hops leaves $self a
# symlink; a chain that changed under us can leave it dangling or a directory, and
# a failed readlink leaves a trailing slash. Each case would take \`pnpm\` from the
# wrong directory — the substitution this script exists to prevent.
if [ -L "$self" ] || [ ! -f "$self" ]; then
  echo "${name}: could not resolve $0 to a regular file within 40 symlink hops." >&2
  exit 1
fi

pnpm=\${self%/*}/pnpm
# The placeholder setup.js replaces with the native binary is not executable, so
# this reports the skipped install script rather than an EACCES from \`exec\`.
if [ ! -x "$pnpm" ]; then
  echo "${name}: pnpm's native binary was not installed next to this script." >&2
  echo "Reinstall @pnpm/exe with its install scripts allowed." >&2
  exit 1
fi

exec "$pnpm"${subcommand} "$@"
`
}
