#!/bin/bash
set -e
set -u

npm run tsc
mv node_modules _node_modules
pnpx npm@4 cache clear

node .scripts/addBundleDependencies
rm -rf node_modules
pnpx npm@4 i --production --legacy-bundling --ignore-scripts
pnpx npm@4 publish --ignore-scripts --tag next
rm -rf node_modules
mv _node_modules node_modules
node .scripts/removeBundleDependencies
