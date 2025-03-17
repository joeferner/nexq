#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

cd "${SCRIPT_DIR}/../packages"
for d in core test store-memory store-sql proto-rest proto-prometheus app; do
  pushd "${d}" > /dev/null
  if grep -q '"test"' package.json; then
    banner "testing ${d}..."
    npm run test
  fi
  popd > /dev/null
done
