#!/bin/bash
set -e
set -u

node .scripts/check
npm run tsc
mv node_modules _node_modules
pnpx npm@4 cache clear

# Regular version release
pnpx npm@4 i --production --ignore-scripts
pnpx npm@4 shrinkwrap
pnpx npm@4 publish --ignore-scripts --tag next
rm npm-shrinkwrap.json

# Bundled version release
node .scripts/addBundleDependencies
rm -rf node_modules
pnpx npm@4 i --production --legacy-bundling --ignore-scripts
pnpx npm@4 publish --ignore-scripts --tag next
rm -rf node_modules
mv _node_modules node_modules
node .scripts/removeBundleDependencies

# Self-installer release
cd .scripts/self-installer
pnpm i
npm publish --tag next
