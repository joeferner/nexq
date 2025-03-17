#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

cd "${SCRIPT_DIR}/../packages"
for d in core test store-memory store-sql proto-rest proto-prometheus app; do
  pushd "${d}" > /dev/null
  if grep -q '"build"' package.json; then
    banner "building ${d}..."
    if [ ! -d node_modules ]; then
      npm install --no-save
    fi
    npm run build
  fi
  popd > /dev/null
done
