#!/bin/bash

set -e;

cd test/packages;
npm_config_registry=http://localhost:4873/;
npm config set "//localhost:4873/:_authToken=h6zsF82dzSCnFsws9nQXtxyKcBY";

cd hello-world-js-bin;
npm publish;
cd ..;

cd pkg-with-bundled-dependencies;
npm install;
npm publish;
cd ..;

cd not-compatible-with-any-os;
npm publish;
cd ..;

cd for-legacy-node;
npm publish;
cd ..;

cd for-legacy-pnpm;
npm publish;
cd ..;

cd circular-deps-1-of-2;
npm publish;
cd ..;

cd circular-deps-2-of-2;
npm publish;
cd ..;

cd dep-of-pkg-with-1-dep;
npm publish;
cd ..;

cd pkg-with-1-dep;
npm publish;
cd ..;

cd install-script-example;
npm publish;
cd ..;

cd pre-and-postinstall-scripts-example;
npm publish;
cd ..;

cd pkg-that-installs-slowly;
npm publish;
cd ..;

cd pkg-that-uses-plugins;
npm publish;
cd ..;

cd plugin-example;
npm publish;
cd ..;

cd test-pnpm-peer-deps;
npm publish;
cd ..;

cd peer-deps-in-child-pkg;
npm publish;
cd ..;

cd sh-hello-world;
npm publish;
cd ..;
