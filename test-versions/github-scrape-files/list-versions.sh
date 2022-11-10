#! /usr/bin/env bash

find github -name '*.yaml' -print0 | xargs -0 grep -h 'Version: ' | tr -d '\r' | sort | uniq

