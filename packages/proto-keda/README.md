see [NexQ](https://github.com/joeferner/nexq)

```yaml
apiVersion: keda.sh/v1alpha1
kind: ScaledObject
metadata:
  name: scaledobject-name
  namespace: scaledobject-namespace
spec:
  scaleTargetRef:
    name: deployment-name
  triggers:
    - type: external
      metadata:
        scalerAddress: nexq.svc.local:7890
        # Name of the queue to monitor
        queueName: queue1
        # Value to start scaling for
        threshold: "100"
        # Target value for activating the scaler (default: 0)
        activationThreshold: "0"
```
