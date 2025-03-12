#!/bin/bash

docker run \
    --rm -it \
    --name nexq-postgres \
    -e POSTGRES_USER=nexq \
    -e POSTGRES_PASSWORD=password \
    -p 5432:5432 \
    postgres
