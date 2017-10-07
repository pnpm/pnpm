#!/bin/bash
set -e
set -u

node .scripts/check
npm run tsc
mv node_modules _node_modules
npx npm@4 cache clear

# Regular version release
npx npm@4 i --production --ignore-scripts
npx npm@4 shrinkwrap
npx npm@4 publish --ignore-scripts --tag next
rm npm-shrinkwrap.json

# Bundled version release
node .scripts/addBundleDependencies
rm -rf node_modules
npx npm@4 i --production --legacy-bundling --ignore-scripts
npx npm@4 publish --ignore-scripts --tag next
rm -rf node_modules
mv _node_modules node_modules
node .scripts/removeBundleDependencies
