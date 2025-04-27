#!/bin/sh
set -e
set -u

if [ $# -eq 0 ]; then
  node packages/server/build/main.js --config /opt/nexq/config/nexq.yml
else
  node packages/server/build/main.js "$@"
fi
