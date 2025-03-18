#!/bin/sh
set -e
set -u

cd "/opt/nexq/"

node app/build/main.js --config /opt/nexq/config/nexq.yml
