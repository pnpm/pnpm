#!/bin/bash
set -e
set -u

node .scripts/addBundleDependencies
npm run tsc
mv node_modules _node_modules
npm i --production --legacy-bundling --ignore-scripts
npm publish --ignore-scripts --tag next
rm -rf node_modules
rm package-lock.json
mv _node_modules node_modules
node .scripts/removeBundleDependencies
