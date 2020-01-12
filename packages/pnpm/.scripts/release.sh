#!/bin/bash
set -e
set -u

pnpm run tsc
npm cache clear --force
publish-packed --tag next --prune
