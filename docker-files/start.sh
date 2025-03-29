#!/bin/sh
set -e
set -u

if [ $# -eq 0 ]; then
  node packages/app/build/main.js --config /opt/nexq/config/nexq.yml
else
  node packages/app/build/main.js "$@"
fi
