import fs from 'fs'
import path from 'path'

const ownDir = import.meta.dirname
const placeholder = 'This file intentionally left blank'
const reinstallHint = 'Reinstall @pnpm/exe with its install scripts allowed.'

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
for (const [name, subcommand] of [['pn', ''], ['pnpx', ' dlx'], ['pnx', ' dlx']]) {
  const file = path.join(ownDir, name)
  try {
    fs.unlinkSync(file)
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
  }
  fs.writeFileSync(file, unixScript(name, subcommand), { mode: 0o755 })
  fs.writeFileSync(path.join(ownDir, name + '.cmd'), cmdScript(name, subcommand))
  fs.writeFileSync(path.join(ownDir, name + '.ps1'), powershellScript(name, subcommand))
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
  echo "${missingBinaryMessage(name)}" >&2
  echo "${reinstallHint}" >&2
  exit 1
fi

exec "$pnpm"${subcommand} "$@"
`
}

function cmdScript (name, subcommand) {
  // The redirection leads each `echo` because cmd.exe strips it from the line
  // without stripping the space in front of it, which a trailing `1>&2` would
  // print as part of the message.
  //
  // cmd.exe seeks by byte offset when it takes a `goto`, so the file has to
  // carry the CRLF endings a batch file is expected to have.
  return `@echo off
if not exist "%~dp0pnpm.exe" goto missing_binary
if exist "%~dp0pnpm.exe\\" goto missing_binary
"%~dp0pnpm.exe"${subcommand} %*
exit /b %errorlevel%

:missing_binary
>&2 echo ${missingBinaryMessage(name)}
>&2 echo ${reinstallHint}
exit /b 1
`.replace(/\n/g, '\r\n')
}

function powershellScript (name, subcommand) {
  return `$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent
$pnpm="$basedir\\pnpm.exe"
if (!(Test-Path -LiteralPath $pnpm -PathType Leaf)) {
  [Console]::Error.WriteLine("${missingBinaryMessage(name)}")
  [Console]::Error.WriteLine("${reinstallHint}")
  exit 1
}
& $pnpm${subcommand} @args
exit $LastExitCode
`
}

function missingBinaryMessage (name) {
  return `${name}: pnpm's native binary was not installed next to this script.`
}
