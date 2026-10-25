import sys, json, base64, socket, os, time, argparse
def handle(line):
    m = json.loads(line); t = time.time()
    body = base64.b64decode(m["request"].get("body_b64", "")).decode("utf-8", "replace")
    labels = (["PII"] if "123-45-6789" in body else []) + (["SECRET"] if "AKIA" in body else [])
    labels = [l for l in labels if l in m.get("labels", labels)]
    sys.stderr.write(f"fake_inspector: {m['request']['method']} {m['request']['url']} -> {labels}\n"); sys.stderr.flush()
    return json.dumps({"v": 1, "id": m["id"], "labels": labels, "findings": [], "backend": "fake", "latency_ms": int((time.time()-t)*1000)}) + "\n"
p = argparse.ArgumentParser(); p.add_argument("--socket"); p.add_argument("--stdio", action="store_true"); a = p.parse_args()
if a.stdio:
    for line in sys.stdin: sys.stdout.write(handle(line)); sys.stdout.flush()
else:
    if os.path.exists(a.socket): os.unlink(a.socket)
    s = socket.socket(socket.AF_UNIX); s.bind(a.socket); s.listen(8)
    while True:
        c, _ = s.accept(); f = c.makefile("rw")
        for line in f: f.write(handle(line)); f.flush()
        c.close()
