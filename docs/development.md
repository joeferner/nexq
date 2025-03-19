
# Testing Postgres

Outside of dev container
```bash
docker network create nexq
docker run --rm -it --network=nexq --name nexq-postgres -e POSTGRES_USER=nexq -e POSTGRES_PASSWORD=password postgres
```

Inside dev container
```bash
cd packages/store-sql
TEST_POSTGRES=1 npm run test:watch
```
