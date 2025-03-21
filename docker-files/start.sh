#!/bin/sh
set -e
set -u

if [ $# -eq 0 ]; then
  node app/build/main.js --config /opt/nexq/config/nexq.yml
else
  node app/build/main.js "$@"
fi
