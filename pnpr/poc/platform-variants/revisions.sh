#!/usr/bin/env bash
# A patched revision through pnpr, installed by pnpm with the registry declared
# `serverType: pnpr` (pnpr's format), undeclared (npm's documents), and with a
# `+r0` override in both. Each successful install is repeated frozen.
# usage: revisions.sh <work-dir>
set -u
HERE=$(dirname "$(realpath "$0")"); BIN=$HERE/../../../target/release
W=$(realpath "$1"); P=http://127.0.0.1:7696
mkdir -p $W/build/orig/package $W/build/patched/package
for v in orig patched; do
  printf '{"name":"revpkg","version":"1.0.0","main":"index.js"}' > $W/build/$v/package/package.json
  echo "module.exports = '$v'" > $W/build/$v/package/index.js
  (cd $W/build/$v && tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -cf ../$v.tar package)
done
cp $HERE/revision-upstream.mjs $W/upstream.mjs
printf 'storage: ./storage\nregistries:\n  revup:\n    type: upstream\n    url: http://127.0.0.1:7695/\n    public: true\ndefaultRegistry: revup\n' > $W/pnpr.yaml
node $W/upstream.mjs 7695 & UP=$!
cd $W
$BIN/pnpr -c pnpr.yaml --listen 127.0.0.1:7696 > $W/pnpr.log 2>&1 & PN=$!
trap 'kill $UP $PN 2>/dev/null' EXIT
for _ in $(seq 100); do curl -sf $P/npm/revpkg >/dev/null && break; sleep 0.1; done
run() { # <name> <declared> <override>
  local d; d=$(mktemp -d -p $W $1.XXXX); cd $d
  echo '{"name":"app","private":true,"dependencies":{"revpkg":"1.0.0"}}' > package.json
  echo "registry=$P/npm/" > .npmrc
  printf 'storeDir: %s/store\ncacheDir: %s/cache\nminimumReleaseAge: 0\n' $d $d > pnpm-workspace.yaml
  [ "$2" = yes ] && printf 'registries:\n  %s/npm/: {serverType: pnpr}\n' $P >> pnpm-workspace.yaml
  [ -n "$3" ] && printf 'overrides:\n  revpkg: "%s"\n' "$3" >> pnpm-workspace.yaml
  $BIN/pnpm install > install.log 2>&1; local rc=$?
  echo "$1: exit=$rc installs '$(node -p 'require("revpkg")' 2>/dev/null)'"
  [ $rc -ne 0 ] && { grep -m1 'revision' install.log; return; }
  grep 'resolution:' pnpm-lock.yaml
  rm -rf node_modules store cache
  $BIN/pnpm install --frozen-lockfile > frozen.log 2>&1
  echo "  frozen: exit=$? installs '$(node -p 'require("revpkg")' 2>/dev/null)'"
}
run declared yes ""
run undeclared no ""
run override-r0 yes "1.0.0+r0"
run override-r0-undeclared no "1.0.0+r0"
