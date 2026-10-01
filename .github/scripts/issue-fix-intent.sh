#!/usr/bin/env bash

# Reads an issue body on stdin and prints whether the author ticked the issue
# form's "I am working on a fix" checkbox: "checked", "unchecked", or
# "missing" when the body has no such checkbox, e.g. an issue filed with
# `gh issue create --body`. Only the checkbox under the form's own
# "Contributing a fix" heading counts, not the same text in another section.

set -euo pipefail

awk '
  { sub(/\r$/, "") }
  /^### / { in_fix = ($0 ~ /^### Contributing a fix[[:space:]]*$/); next }
  in_fix && /^- \[[ xX]\] I am working on a fix[[:space:]]*$/ {
    seen = 1
    if ($0 !~ /^- \[ \]/) checked = 1
  }
  END { print checked ? "checked" : seen ? "unchecked" : "missing" }
'
