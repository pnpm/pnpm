#!/usr/bin/env bash
# Publish small packages with platform variants to a hosted pnpr, install them
# with pnpm from pnpr's format, and repeat each install frozen. Each package's
# variants declare their platforms in their own package.json; the invariant
# cases that must be rejected are published too.
# usage: variants.sh <work-dir> [<bin-dir>]
set -u
HERE=$(dirname "$(realpath "$0")"); BIN=$(realpath "${2:-$HERE/../../../target/release}")
W=$(realpath "$1"); P=http://127.0.0.1:7697
mkdir -p $W
node $HERE/mkinvariants.mjs $W/fixtures > /dev/null
cat > $W/pnpr.yaml <<'EOF'
storage: ./storage
auth:
  htpasswd:
    file: ./htpasswd
    maxUsers: 1
registries:
  local:
    type: hosted
    packages:
      '@variants-poc/*':
        publish: $authenticated
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  main:
    type: router
    sources: [local, npmjs]
defaultRegistry: main
EOF
cd $W
$BIN/pnpr -c pnpr.yaml --listen 127.0.0.1:7697 > $W/pnpr.log 2>&1 & PN=$!
trap 'kill $PN 2>/dev/null' EXIT
for _ in $(seq 100); do curl -sf $P/-/pnpr/v0/ready >/dev/null 2>&1 && break; curl -s $P/npm/ >/dev/null 2>&1 && break; sleep 0.1; done
TOKEN=$(curl -s -X PUT $P/npm/-/user/org.couchdb.user:poc -H 'content-type: application/json' \
  -d '{"name":"poc","password":"poc-password-1"}' | node -p 'JSON.parse(require("fs").readFileSync(0)).token')
for f in $W/fixtures/*.variants.json; do
  node $HERE/publish.mjs $P/npm/ "$TOKEN" "$f" 2>&1 | cut -c1-200
done
run() { # <package>
  local d; d=$(mktemp -d -p $W install.XXXX); cd $d
  echo "{\"name\":\"app\",\"private\":true,\"dependencies\":{\"$1\":\"1.0.0\"}}" > package.json
  echo "registry=$P/npm/" > .npmrc
  printf 'storeDir: %s/store\ncacheDir: %s/cache\nminimumReleaseAge: 0\n' $d $d > pnpm-workspace.yaml
  printf 'registries:\n  %s/npm/: {serverType: pnpr}\n' $P >> pnpm-workspace.yaml
  $BIN/pnpm install > install.log 2>&1; local rc=$?
  echo "$1: exit=$rc links '$(node -p "require('$1')" 2>/dev/null)'"
  [ $rc -ne 0 ] && { tail -3 install.log; return; }
  sed -n '/^packages:/,$p' pnpm-lock.yaml
  rm -rf node_modules store cache
  $BIN/pnpm install --frozen-lockfile > frozen.log 2>&1
  echo "  frozen: exit=$? links '$(node -p "require('$1')" 2>/dev/null)'"
  [ -n "${2:-}" ] && echo "  $2 from the package: $(node -p "require.resolve('$2', {paths: [require('path').dirname(require.resolve('$1/package.json'))]}).includes('$2')" 2>&1 | tail -1)"
  $BIN/pnpm add is-odd@3.0.1 > add.log 2>&1
  echo "  add: exit=$? variant snapshots kept: $(grep -c '/variants/' pnpm-lock.yaml)"
}
run @variants-poc/inv-ok
run @variants-poc/inv-deps-differ is-number
