#!/bin/bash

set -e;

if [ -d node_modules ]; then
  echo "moving the current node_modules to a temporal location";
  mv node_modules .tmp/node_modules;
fi

echo "install just the dependencies and don't run any pre/post install scripts";
npm install --production --ignore-scripts;

echo "move node_modules/ to lib/";
mv node_modules lib/node_modules;

echo "remove the dependencies section from package.json";
node .scripts/hide_deps;

echo "publish pnpm $1";
if [[ $1 ]]; then
  npm publish --tag $1;
else
  npm publish;
fi

set +e;

echo "return the dependencies section to package.json";
node .scripts/unhide_deps;

if [ -d .tmp/node_modules ]; then
  echo "remove lib/node_modules";
  rm -rf lib/node_modules;

  echo "return the initial node_modules";
  mv .tmp/node_modules node_modules;
else
  echo "move node_modules back to the root";
  mv lib/node_modules node_modules;
fi
