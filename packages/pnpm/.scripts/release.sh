#!/bin/bash
set -e
set -u

npm run tsc
pnpx npm@4 cache clear
pnpx publish-packed@2.1.1 --tag next --prune
