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
