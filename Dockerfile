FROM node:22-alpine

ENV NODE_ENV=production

WORKDIR /opt/nexq/core
RUN \
    --mount=type=bind,source=packages/core/package.json,target=package.json \
    --mount=type=bind,source=packages/core/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

WORKDIR /opt/nexq/store-memory
RUN \
    --mount=type=bind,source=packages/store-memory/package.json,target=package.json \
    --mount=type=bind,source=packages/store-memory/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

WORKDIR /opt/nexq/store-sql
RUN \
    --mount=type=bind,source=packages/store-sql/package.json,target=package.json \
    --mount=type=bind,source=packages/store-sql/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

WORKDIR /opt/nexq/proto-rest
RUN \
    --mount=type=bind,source=packages/proto-rest/package.json,target=package.json \
    --mount=type=bind,source=packages/proto-rest/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

WORKDIR /opt/nexq/proto-prometheus
RUN \
    --mount=type=bind,source=packages/proto-prometheus/package.json,target=package.json \
    --mount=type=bind,source=packages/proto-prometheus/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

WORKDIR /opt/nexq/app
RUN \
    --mount=type=bind,source=packages/app/package.json,target=package.json \
    --mount=type=bind,source=packages/app/package-lock.json,target=package-lock.json \
    --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev

COPY packages/core /opt/nexq/core
COPY packages/store-memory /opt/nexq/store-memory
COPY packages/store-sql /opt/nexq/store-sql
COPY packages/proto-rest /opt/nexq/proto-rest
COPY packages/proto-prometheus /opt/nexq/proto-prometheus
COPY packages/app /opt/nexq/app

COPY packages/app/config/nexq.yml /opt/nexq/config/nexq.yml
COPY docker-files/start.sh /opt/nexq/start

WORKDIR /opt/nexq/
USER node

ENTRYPOINT ["/bin/sh"]
CMD ["./start", "--help"]
