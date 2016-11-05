#!/bin/bash

set -e;

cd test/packages;
npm_config_registry=http://localhost:4873/;

cd hello-world-js-bin;
npm publish;
cd ..;

cd pkg-with-bundled-dependencies;
npm install;
npm publish;
