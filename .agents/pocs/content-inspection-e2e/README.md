# PROTOTYPE — throwaway: content inspection at the egress gateway

Strands Box can deny an outbound HTTP request because of what it carries. A policy asks for a
classification, Box sends the decrypted request to an inspector you choose, and Dogwood decides on
the labels the inspector returns.

```cedar
forbid (principal, action == Box::Action::"http:request", resource)
when { Inspection::Content(["PII", "SECRET"]).labels.containsAny(["PII", "SECRET"]) };
```

This is a prototype on branch `feature/prototype-content-inspection`. It is not a shipped feature,
and nothing here is a stable interface.

## How it works

```text
agent (curl) ──HTTPS──▶ egress gateway (MITM, body buffered)
                              │ policy uses Inspection::Content?  no ─▶ decide as usual
                              │ yes
                              ▼
                        inspector  ◀── NDJSON: method, url, headers (auth redacted), body
                              │      ──▶ {"labels":["PII"], "findings":[...]}
                              ▼
                        Dogwood decides with the labels ─▶ 200 or 403
```

- The inspector is any program that speaks the protocol. `../content-inspector/inspector.py`
  wraps three backends: `regex`, `presidio`, and `decider` (Strands Decider, a local 2B model).
- Box reaches the inspector in one of two ways:
  - **socket**: the inspector runs outside the box, started by you, and can serve many boxes.
  - **command**: Box starts the inspector once, in a contained leaf, and talks to it on stdio.
    `gpu = true` lets that leaf use Metal on macOS.
- If the inspector fails, times out, or the body is over 1 MiB, the request is denied.
- If the policy never calls `Inspection::Content`, the inspector is not called at all.
- `authorization`, `proxy-authorization`, and `cookie` values are redacted before the inspector sees
  them.

## Configuration

```toml
[inspection.content]
provider = "local"                 # the only value in the prototype
timeout_ms = 2000                  # per request; use 120000 for Decider on CPU

# Either: connect to an inspector that is already running.
socket = "/abs/path/inspector.sock"

# Or: Box starts it in a leaf.
# command = ["/path/to/python3", "/path/to/inspector.py", "--stdio", "--backend", "regex"]
# env = { HOME = "/Users/you" }
# gpu = true                       # macOS only: Metal for the leaf, and only this leaf
# [inspection.content.filesystem]
# read = ["/path/to/python/prefix", "/path/to/content-inspector"]
```

## Set up (macOS, Apple Silicon)

From the repository root:

```sh
cargo build -p strands-box -p strands-box-containment --all-features

cd .agents/pocs/content-inspector
python3 -m venv .venv
.venv/bin/pip install torch strands-decider
.venv/bin/python -c "import torch; print('mps:', torch.backends.mps.is_available())"
cd -
```

The first Decider run downloads about 4.4 GB of weights from Hugging Face. No token or AWS
credentials are needed.

## Demo 1: a shared inspector on the GPU (socket)

**Terminal 1** — start the inspector. Decider takes about 20 s to load; wait for the
`decider loaded ... on mps` line.

```sh
cd .agents/pocs/content-inspector
.venv/bin/python -I inspector.py --socket /tmp/inspector.sock --backend decider --device mps
```

Use `--backend regex` instead for an instant start and ~1 ms answers.

**Terminal 2** — write a box. Run this from the repository root.

```sh
export DEMO="$(cd /tmp && pwd -P)/inspection-demo"
mkdir -p "$DEMO/ws" "$DEMO/home"
cp .agents/pocs/content-inspection-e2e/policy.dw "$DEMO/policy.dw"
cat > "$DEMO/box.toml" <<EOF
name = "inspection-demo"
box_dir = "$DEMO/state"
policy = "$DEMO/policy.dw"

[agent]
command = ["/usr/bin/curl"]
workspace = "$DEMO/ws"
env = { PATH = "/usr/bin:/bin", HOME = "$DEMO/home" }
[agent.filesystem]
read_file = ["/private/etc/ssl/openssl.cnf"]

[inspection.content]
provider = "local"
socket = "/private/tmp/inspector.sock"
timeout_ms = 10000
EOF
cat "$DEMO/policy.dw"
```

On Linux, leave out the `[agent.filesystem]` table, use `/tmp/inspector.sock` as the socket, and
`export EXPERIMENTAL_LINUX_SUPPORT=TRUE`.

The policy permits HTTPS to `httpbin.org` and forbids any request labelled `PII` or `SECRET`.

Each command below runs `curl` inside a new box. The arguments after `--` go to `curl`.

```sh
BOX=./target/debug/strands-box
post() { $BOX run --config "$DEMO/box.toml" -- -sS -w '\nstatus=%{http_code}\n' \
           -X POST -H 'content-type: application/json' --data "$1" https://httpbin.org/post; }

post '{"msg":"hello world"}'                               # 200, labels=[]
post '{"name":"Jane Doe","ssn":"123-45-6789"}'             # 403, labels=["PII"]
post '{"aws_access_key_id":"AKIAIOSFODNN7EXAMPLE"}'        # 403, labels=["SECRET"]
```

What to point at on screen:

- the clean request returns httpbin's echo and `status=200`;
- the box prints one line per inspected request, for example
  `inspection: POST https://httpbin.org/post labels=["PII"] backend=decider latency=1400ms`;
- the PII and secret requests get `status=403` from the gateway, and the request never leaves
  the machine;
- Terminal 1 logs each request it classified.

**Redaction.** Credentials in headers are not sent to the inspector:

```sh
$BOX run --config "$DEMO/box.toml" -- -sS -o /dev/null -w 'status=%{http_code}\n' \
  -H 'authorization: Bearer ghp_notarealtoken' https://httpbin.org/get     # 200
```

**No rule, no inspection.** Switch to the control policy, and the same SSN request passes. No
`inspection:` line is printed, and Terminal 1 stays quiet.

```sh
cp .agents/pocs/content-inspection-e2e/policy-noinspect.dw "$DEMO/policy.dw"
post '{"ssn":"123-45-6789"}'                               # 200
cp .agents/pocs/content-inspection-e2e/policy.dw "$DEMO/policy.dw"
```

**Fail closed.** Stop the inspector in Terminal 1 (Ctrl-C), and send the clean request again.
It gets 403. The response body says `http:request gate: content inspection failed: connect ...`,
and the box prints `inspection: POST ... refused: inspector failed: connect ...`.

## Demo 2: Box starts the inspector in a contained leaf, on the GPU

The leaf needs read grants for the Python interpreter, the venv, and the model cache, so the e2e
script writes this config for you. It runs a clean POST (200), an SSN POST (403), and an SSN POST
under the control policy (200, inspector not called).

```sh
cd .agents/pocs/content-inspection-e2e
DEVICE=mps ./run-e2e.sh stdio decider
cat box-stdio-b-pii.toml          # the generated config: command, filesystem grants, gpu = true
```

On screen, the startup disclosure lists the leaf's grants, including
`gpu  Metal (AGX, IOSurface, MTLCompilerService)`, and then
`inspection: content inspector leaf started (pid ...)`.

To show that the GPU grant is what makes this work, run it without the grant. Case a fails with
403, and the inspector's stderr shows `The MPS backend is supported on macOS 14.0+`:

```sh
GPU=0 DEVICE=mps ./run-e2e.sh stdio decider
```

`gpu = true` adds three Seatbelt rules to this leaf only, never to the agent or another tool:

```scheme
(allow iokit-open-user-client (iokit-user-client-class "AGXDeviceUserClient" "IOSurfaceRootUserClient"))
(allow sysctl-read (sysctl-name "hw.memsize"))
(allow mach-lookup (xpc-service-name "com.apple.MTLCompilerService"))
```

## All runs

```sh
cd .agents/pocs/content-inspection-e2e
./run-e2e.sh socket regex
./run-e2e.sh stdio  regex
DEVICE=mps ./run-e2e.sh socket decider
DEVICE=mps ./run-e2e.sh stdio  decider
```

On Linux, set nothing extra: the script exports `EXPERIMENTAL_LINUX_SUPPORT=TRUE`. `gpu = true`
is accepted there and does nothing.

## Measured latency per request

| Backend | Where | Latency |
|---|---|---|
| regex | any | 0–4 ms |
| Decider | M1 GPU, socket | 1.2–1.4 s |
| Decider | M1 GPU, leaf | 2.0–2.2 s |
| Decider | M1 CPU | 1.7–2.3 s |
| Decider | Linux aarch64 CPU | ~3.2 s |

## Known gaps

- A denial from inspection is not written to the policy history.
- The leaf is not restarted if it exits; every request after that is denied.
- `timeout_ms` is per read on the socket, not an end-to-end deadline.
- The inspector only sees request bodies. Responses, MCP tool arguments, and file writes are not
  inspected.
- Decider's accuracy is not measured. In one smoke test it missed a GitHub token that the regex
  backend caught.
