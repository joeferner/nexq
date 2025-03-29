# Integration Test

Outside dev container

```bash
./scripts/docker-build.sh
minikube -p nexq-keda-it start
minikube -p nexq-keda-it image load --overwrite nexq:latest
helm upgrade --install nexq --set image.tag=latest,image.tag=latest helm-chart/

minikube -p nexq-keda-it delete
```
