#!/bin/bash

DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${DIR}" || exit 1

if [ ! -d node_modules ]; then
  npm ci
fi

npx jest --runInBand --verbose "$@"
