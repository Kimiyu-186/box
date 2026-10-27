#!/usr/bin/env python3
"""PROTOTYPE client: send capabilities and canned or custom classify requests (protocol 0.2)."""

import argparse
import base64
import json
import os
import socket
import ssl
import time
import urllib.request

CASES = {
    "clean": ("POST", "https://api.github.com/repos/o/r/issues", '{"title":"Build is green","body":"CI passed on main."}'),
    "pii": ("POST", "https://api.example.com/users?ref=signup", '{"name":"John Doe","email":"john.doe@example.com","ssn":"123-45-6789"}'),
    "secret": ("POST", "https://paste.example.com/new", "aws_access_key_id = AKIAIOSFODNN7EXAMPLE\naws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n"),
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--socket", default=os.environ.get("INSPECTOR_SOCKET"))
    ap.add_argument("--https", metavar="URL", help="e.g. https://127.0.0.1:8443/v1/classify")
    ap.add_argument("--cafile", help="CA bundle for --https")
    ap.add_argument("--token", default=os.environ.get("INSPECTOR_TOKEN"))
    ap.add_argument("--labels", default="PII,SECRET")
    ap.add_argument("--body", help="custom body (overrides canned cases)")
    ap.add_argument("--url", default="https://api.example.com/x")
    ap.add_argument("--dns", metavar="NAME", action="append", default=[], help="send a dns subject")
    ap.add_argument("--connect", metavar="NAME=IP", action="append", default=[], help="send a connect subject")
    ap.add_argument("cases", nargs="*", default=None)
    a = ap.parse_args()
    if a.cases is None or not a.cases:
        a.cases = [] if (a.dns or a.connect) else list(CASES)
    reqs = [("custom", ("POST", a.url, a.body))] if a.body is not None else [(c, CASES[c]) for c in a.cases]
    msgs = [("capabilities", {"v": "0.2", "id": "caps", "op": "capabilities", "params": {}})]
    for name, (method, url, body) in reqs:
        msgs.append((name, {
            "v": "0.2",
            "id": name,
            "op": "classify",
            "params": {
                "subject": {
                    "kind": "http",
                    "method": method,
                    "url": url,
                    "headers": [["content-type", "application/json"], ["authorization", "[redacted]"]],
                    "body_b64": base64.b64encode(body.encode()).decode(),
                },
                "labels": a.labels.split(","),
                "context": {"gateway": "client.py", "box": "none", "caller": "agent"},
            },
        }))
    for name in a.dns:
        msgs.append((f"dns {name}"[:12], {"v": "0.2", "id": f"dns-{name}", "op": "classify", "params": {
            "subject": {"kind": "dns", "name": name, "port": 443},
            "labels": ["MALICIOUS_DOMAIN", "TUNNELING"]}}))
    for pair in a.connect:
        name, _, ip = pair.partition("=")
        msgs.append((f"conn {ip}"[:12], {"v": "0.2", "id": f"connect-{pair}", "op": "classify", "params": {
            "subject": {"kind": "connect", "name": name, "ip": ip, "port": 443},
            "labels": ["MALICIOUS_IP"]}}))
    if a.https:
        ctx = ssl.create_default_context(cafile=a.cafile)
        def send(msg):
            req = urllib.request.Request(a.https, data=json.dumps(msg).encode(), method="POST",
                                         headers={"Content-Type": "application/json",
                                                  **({"Authorization": f"Bearer {a.token}"} if a.token else {})})
            try:
                with urllib.request.urlopen(req, context=ctx) as r:
                    return f"http={r.status} {r.read().decode()}"
            except urllib.error.HTTPError as e:
                return f"http={e.code} {e.read().decode()}"
    else:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.connect(a.socket)
        f = s.makefile("rwb")
        def send(msg):
            f.write((json.dumps(msg) + "\n").encode())
            f.flush()
            return f.readline().decode().strip()
    for name, msg in msgs:
        t = time.monotonic()
        resp = send(msg)
        print(f"{name:12} rtt={round((time.monotonic() - t) * 1000)}ms {resp}")


if __name__ == "__main__":
    main()
