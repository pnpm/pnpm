#!/bin/bash
set -e
set -u

pnpm run tsc
npm cache clear
publish-packed --tag next --prune
