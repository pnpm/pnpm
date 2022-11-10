#! /usr/bin/env bash

set -x

# https://github.com/pnpm/spec/tree/master/lockfile

# get versions:
# git checkout main; git tag -l 'v*' | grep -v - | sed 's/^v//'

# broken pnpm versions: 'pnpm install' has no effect
#3.4 1.23.0 10
#3.3 1.22.0 10
#3.2 1.18.1 10
#3.1 1.17.2 10
#3.1 1.17.0 10 -> ERR_PNPM_NO_MATCHING_VERSION  No matching version found for pnpm@1.17.0 -> 1.17.2

# note: this 'read' will exit with 'set -e'
# columns: lockVersion pnpmVersion nodeVersion
read -r -d "" versionMap <<"EOF"
5.3 6.0.0 16
5.2 5.10.0 16
5.1 3.5.0 10
5.0 3.0.0 10
3.9 2.13.3 10
3.8 2.8.0 10
3.7 2.0.0 10
3.6 1.43.0 10
3.5 1.40.0 10
3.0 1.0.0 10
2.0 0.62.0 10
EOF

# 5.0 and after -> pnpm-lock.yaml
# 3.9 and before -> shrinkwrap.yaml

# note: 3.9 == 4.0
#4.0 2.17.0 10
#3.9 2.17.0 10

set -e

echo "versionMap = $versionMap"

# old versions require node-10
# node-12: TypeError: cb.apply is not a function

while read -r versionMapLine
do
  lockVersion=$(echo $versionMapLine | cut -d' ' -f1)
  pnpmVersion=$(echo $versionMapLine | cut -d' ' -f2)
  nodeVersion=$(echo $versionMapLine | cut -d' ' -f3)
  echo "lockVersion = $lockVersion"
  echo "pnpmVersion = $pnpmVersion"
  echo "nodeVersion = $nodeVersion"

  #version=3.0.0
  [ -d $lockVersion ] || mkdir $lockVersion
  [ -d $lockVersion/pnpm ] || mkdir $lockVersion/pnpm
  if [ ! -d $lockVersion/pnpm/node_modules/pnpm ]; then
    (
      cd $lockVersion/pnpm
      set -x
      pnpm init -y
      pnpm i "pnpm@$pnpmVersion"
    )
  fi
  pnpm_js=$(cat $lockVersion/pnpm/node_modules/pnpm/package.json | jq -r .bin.pnpm)
  pnpm_js=$(realpath $lockVersion/pnpm/node_modules/pnpm/$pnpm_js)
  if (echo "$pnpm_js" | grep -q ' ');
  then
    echo "error: path must not contain spaces: $pnpm_js"
    exit 1
  fi
  if [ ! -e $lockVersion/pnpm.sh ]; then
    #echo "#! /usr/bin/env bash" >$lockVersion/pnpm.sh
    echo "#! /usr/bin/env cached-nix-shell" >$lockVersion/pnpm.sh
    echo "#! nix-shell -i bash -p nodejs-${nodeVersion}_x" >>$lockVersion/pnpm.sh
    echo "node \"$pnpm_js\" \"\$@\"" >>$lockVersion/pnpm.sh
    chmod +x $lockVersion/pnpm.sh
  fi

  if [[ ! -e $lockVersion/test/shrinkwrap.yaml && ! -e $lockVersion/test/pnpm-lock.yaml ]]; then
    [ -d $lockVersion/test ] || mkdir $lockVersion/test
    (
      cd $lockVersion/test
      nix-shell -p nodejs-${nodeVersion}_x --run "( set -x; node $pnpm_js init -y && node $pnpm_js install cowsay )"
    )
  fi

done <<<"$versionMap"
