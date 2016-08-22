#!/bin/bash

if [ -d node_modules ]; then
  echo "rename the current node_modules";
  mv node_modules prev_node_modules;
fi

set -e; # if installation will fail, fail the whole script
echo "install just the dependencies and don't run any pre/post install scripts";
npm install --production --ignore-scripts;
set +e;

publish () {
  if [[ $1 ]]; then
    npm publish --tag $1;
  else
    npm publish;
  fi
}

echo "publish pnpm $1";
publish;

echo "rename node_modules to cached_node_modules";
node .scripts/rename node_modules cached_node_modules;

echo "remove the dependencies section from package.json";
node .scripts/hide_deps;

echo "publish pnpm-rocket $1";
publish;

echo "return the dependencies section to package.json";
node .scripts/unhide_deps;

if [ -d prev_node_modules ]; then
  echo "remove cached_node_modules";
  rm -rf cached_node_modules;

  echo "return the initial node_modules";
  mv prev_node_modules node_modules;
else
  echo "rename cached_node_modules to node_modules";
  mv cached_node_modules node_modules;
fi
