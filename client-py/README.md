# airbus-client

Python SDK for the [Airbus](https://github.com/ghhwer/airbus) JSON-RPC event bus.

```bash
pip install airbus-client
```

```python
from airbus_client import RpcClient, Queue

client = RpcClient()  # AIRBUS_URL / AIRBUS_HOST:AIRBUS_PORT
assert client.ping() == "pong"
```

The daemon is distributed separately (container image or `cargo`/`make build`).
See the [repository README](https://github.com/ghhwer/airbus) for run instructions.
