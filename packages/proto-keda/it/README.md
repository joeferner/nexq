# Integration Test

Outside dev container in the root of nexq project

```bash
./scripts/docker-build.sh

docker build -f packages/proto-keda/it/Dockerfile --tag nexq-proto-keta-it:latest packages/proto-keda/it/

minikube -p nexq-keda-it start

# load test image
minikube -p nexq-keda-it image load nexq-proto-keta-it:latest

# load and start nexq
minikube -p nexq-keda-it image load nexq:latest
helm upgrade --install nexq --set image.tag=latest,image.tag=latest helm-chart/

# install keda
helm repo add kedacore https://kedacore.github.io/charts
helm repo update
helm install keda kedacore/keda --namespace keda --create-namespace

# install test deployment
helm upgrade --install nexq-proto-keda-it packages/proto-keda/it/helm-chart

minikube -p nexq-keda-it delete
```
