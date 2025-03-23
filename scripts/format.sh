#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

cd "${SCRIPT_DIR}/../"
for d in packages/core packages/test packages/store-memory packages/store-sql packages/proto-rest packages/proto-prometheus packages/app test/it; do
  pushd "${d}" > /dev/null
  banner "formatting ${d}..."
  if [ ! -d node_modules ]; then
    npm install --no-save
  fi
  npm run format
  npm run lint
  popd > /dev/null
done
