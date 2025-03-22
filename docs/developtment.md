# VSCode

Before opening project in container you must create a network

```bash
docker network create nexq
```

After running this you can Ctrl+Shift+P "Reopen in container"

# Initializing the project

```bash
./scripts/build.sh
```

# Before committing

```bash
./scripts/precommit.sh
```
