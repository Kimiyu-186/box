import sys, json, base64, socket, os, argparse
LABELS = ["PII", "SECRET"]
def handle(line):
    m = json.loads(line); rid = m.get("id")
    if m.get("op") == "capabilities":
        result = {"versions": ["0.2"], "inspector": {"name": "fake", "version": "0"},
                  "subjects": {"http": {"labels": LABELS, "max_body_bytes": 1 << 20}}}
    else:
        s = m["params"]["subject"]
        body = base64.b64decode(s.get("body_b64", "")).decode("utf-8", "replace")
        labels = (["PII"] if "123-45-6789" in body else []) + (["SECRET"] if "AKIA" in body else [])
        labels = [l for l in labels if l in m["params"]["labels"]]
        sys.stderr.write(f"fake_inspector: {s['method']} {s['url']} headers={s['headers']} -> {labels}\n"); sys.stderr.flush()
        result = {"labels": labels}
    return json.dumps({"v": "0.2", "id": rid, "result": result}) + "\n"
p = argparse.ArgumentParser(); p.add_argument("--socket"); p.add_argument("--stdio", action="store_true"); a = p.parse_args()
if a.stdio:
    for line in sys.stdin: sys.stdout.write(handle(line)); sys.stdout.flush()
else:
    if os.path.exists(a.socket): os.unlink(a.socket)
    s = socket.socket(socket.AF_UNIX); s.bind(a.socket); os.chmod(a.socket, 0o600); s.listen(8)
    while True:
        c, _ = s.accept(); f = c.makefile("rw")
        for line in f: f.write(handle(line)); f.flush()
        c.close()
