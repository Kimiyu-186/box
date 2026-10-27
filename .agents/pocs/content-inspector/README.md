# PROTOTYPE — throwaway: content inspector for the Box egress gateway

Answers one question: can one inspector protocol (`../egress-classification-protocol/SPEC.md`, draft 0.2) front several classification backends
(regex, Presidio, Strands Decider), and what does each cost per request? No tests. Do not build on it.

## Setup (Linux aarch64, CPU only)

```sh
cd .agents/pocs/content-inspector
python3 -m venv .venv
.venv/bin/pip install torch --index-url https://download.pytorch.org/whl/cpu
.venv/bin/pip install strands-decider presidio-analyzer
.venv/bin/python -m spacy download en_core_web_sm
```

On the first run, the Decider weights (`StrandsAgents/strands-decider-2B-hobson-v19` plus the
`Qwen/Qwen3.5-2B-Base` torso, about 4.4 GB) download from Hugging Face. They need no token,
no Bedrock, and no AWS credentials.

## macOS with a GPU (untested)

```sh
python3 -m venv .venv
.venv/bin/pip install torch strands-decider     # the default macOS wheel has MPS
cd ../content-inspection-e2e
DEVICE=mps ./run-e2e.sh unix decider            # start with unix mode; stdio leaf is untested on macOS
```

## Run

```sh
# unix socket (concurrent connections, many requests per connection)
.venv/bin/python -I inspector.py --socket /tmp/insp.sock --backend regex|presidio|decider
.venv/bin/python -I client.py --socket /tmp/insp.sock            # clean, pii, secret
.venv/bin/python -I client.py --socket /tmp/insp.sock --body 'my ssn is 123-45-6789'

# https on 127.0.0.1 only; the token comes from --token or $INSPECTOR_TOKEN
.venv/bin/python -I inspector.py --https 127.0.0.1:8443 --cert leaf.pem --key leaf.key --token T
.venv/bin/python -I client.py --https https://127.0.0.1:8443/v1/classify --cafile ca.pem --token T

# stdio (Box launches it in a leaf): one message per line on stdin/stdout; all logs go to stderr
.venv/bin/python -I inspector.py --stdio --backend regex < requests.ndjson
```

Decider flags: `--device cpu|cuda|mps|mlx`, `--threshold 0.5`, `--max-state-chars 16384`.

Every backend also answers `dns` (`MALICIOUS_DOMAIN`, `TUNNELING`) and `connect` (`MALICIOUS_IP`)
subjects, with `cache_ttl_s: 300`. `TUNNELING` is a heuristic: a name over 100 characters, a label
of 30 or more characters with Shannon entropy of 3.5 or more, or two hex- or base32-looking labels.
`MALICIOUS_DOMAIN` matches the name or a parent against a built-in list (`evil.example`,
`malware.testing.google.test`, ...) and `--domain-blocklist FILE`. `MALICIOUS_IP` matches
`--ip-blocklist FILE` (one address or CIDR per line). Both files are re-read when they change.
`client.py --dns NAME --connect NAME=IP` sends those subjects.

## Protocol notes for the Box side

- **Responses can arrive out of order.** Requests on one connection (or on stdin) run
  concurrently, so correlate them by `id`.
- `latency_ms` counts time spent waiting for the backend. For Decider that includes queueing
  behind the per-engine lock.
- Bad JSON gives `{"id": null, "error": {"code": "invalid_request", ...}}`. A line longer than 16 MiB gives an error and closes the connection.
- In stdio mode, fd 1 is pointed at stderr after startup, so library prints cannot corrupt the stream.
- Decider gives no span, so its finding has `kind:"model"`, and the evidence is the probability,
  never the content. Presidio is PII only.

## Long-lived, model-backed operation

- Load the model once at startup and run a warm-up inference before accepting requests, so the
  first real request does not pay about 15 s of cold load plus first-forward cost. Then keep the
  process alive across requests and boxes. Restarting per request is not viable.
- One engine per process, serialised by a lock. Throughput then equals 1 / per-request latency
  (about 0.3 req/s on this CPU). Batching questions from many requests into one `engine.ask` is
  the scaling lever, and the engine already batches questions within a request.
- Put a timeout and a fail-closed default on the gateway side. A model backend can stall.
- A GPU (CUDA) or Apple MLX brings latency down to the claimed 115–150 ms. CPU on aarch64 is
  about 1.5 s per question, because `flash-linear-attention` is unavailable and the reference
  kernel runs instead.

## One shared daemon vs one per box

- **Shared daemon:** one copy of 4.4 GB of weights and a few GB of RAM, warm all the time, so
  it suits the model backends. The costs: it sees plaintext request bodies from every box, so it
  is a cross-box trust point and must be hardened like a trusted process. One box can starve the
  others (it needs a per-box queue or fairness). It also needs a stable socket path that each
  box's gateway can reach, and callers need authentication (peer creds).
- **Per box (for example a `--stdio` leaf):** isolation and a lifecycle tied to the box, with no
  cross-box data. This is cheap for regex and Presidio. For Decider it multiplies RAM and cold
  start by the number of boxes, which is acceptable for one or two boxes and not for a fleet.
- A likely split: run regex in-process or per box, and run model backends as a shared,
  hardened local daemon or a remote vendor behind the same protocol.
