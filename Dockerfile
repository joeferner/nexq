FROM node:22-alpine

ENV NODE_ENV=production

WORKDIR /opt/nexq
RUN \
    --mount=type=bind,source=packages/app/package.json,target=packages/app/package.json \
    --mount=type=bind,source=packages/core/package.json,target=packages/core/package.json \
    --mount=type=bind,source=packages/proto-keda/package.json,target=packages/proto-keda/package.json \
    --mount=type=bind,source=packages/proto-prometheus/package.json,target=packages/proto-prometheus/package.json \
    --mount=type=bind,source=packages/proto-rest/package.json,target=packages/proto-rest/package.json \
    --mount=type=bind,source=packages/store-memory/package.json,target=packages/store-memory/package.json \
    --mount=type=bind,source=packages/store-sql/package.json,target=packages/store-sql/package.json \
    --mount=type=bind,source=package.json,target=package.json \
    --mount=type=bind,source=package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm install --workspaces --no-save

COPY packages/core /opt/nexq/packages/core
COPY packages/store-memory /opt/nexq/packages/store-memory
COPY packages/store-sql /opt/nexq/packages/store-sql
COPY packages/proto-rest /opt/nexq/packages/proto-rest
COPY packages/proto-keda /opt/nexq/packages/proto-keda
COPY packages/proto-prometheus /opt/nexq/packages/proto-prometheus
COPY packages/app /opt/nexq/packages/app

COPY docker-files/start.sh /opt/nexq/start

WORKDIR /opt/nexq/
USER node

ENTRYPOINT ["/bin/sh"]
CMD ["./start"]
