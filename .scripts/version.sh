#!/bin/bash
set -e
set -u

node .scripts/afterVersionBump
git add .scripts/self-installer/package.json
