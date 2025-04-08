#!/bin/sh
set -e
set -u

node packages/tui/build/main.js "$@"
