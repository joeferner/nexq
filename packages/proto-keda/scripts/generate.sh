#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

rm -rf src/generated
mkdir -p src/generated
npx grpc_tools_node_protoc \
  --plugin=../../node_modules/.bin/protoc-gen-ts_proto \
  --ts_proto_opt=esModuleInterop=true,importSuffix=.js,outputServices=grpc-js \
  --ts_proto_out=./src/generated \
  --proto_path=. \
  ExternalScaler.proto
