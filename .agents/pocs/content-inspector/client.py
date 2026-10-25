#!/usr/bin/env python3
"""PROTOTYPE client: send canned or custom requests to the inspector socket."""

import argparse
import base64
import json
import os
import socket
import time

CASES = {
    "clean": ("POST", "https://api.github.com/repos/o/r/issues", '{"title":"Build is green","body":"CI passed on main."}'),
    "pii": ("POST", "https://api.example.com/users?ref=signup", '{"name":"John Doe","email":"john.doe@example.com","ssn":"123-45-6789"}'),
    "secret": ("POST", "https://paste.example.com/new", "aws_access_key_id = AKIAIOSFODNN7EXAMPLE\naws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n"),
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--socket", default=os.environ.get("INSPECTOR_SOCKET"))
    ap.add_argument("--labels", default="PII,SECRET")
    ap.add_argument("--body", help="custom body (overrides canned cases)")
    ap.add_argument("--url", default="https://api.example.com/x")
    ap.add_argument("cases", nargs="*", default=list(CASES))
    a = ap.parse_args()
    reqs = [("custom", ("POST", a.url, a.body))] if a.body is not None else [(c, CASES[c]) for c in a.cases]
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(a.socket)
    f = s.makefile("rwb")
    for name, (method, url, body) in reqs:
        msg = {
            "v": 1,
            "id": name,
            "labels": a.labels.split(","),
            "request": {
                "method": method,
                "url": url,
                "headers": [["content-type", "application/json"]],
                "body_b64": base64.b64encode(body.encode()).decode(),
            },
        }
        t = time.monotonic()
        f.write((json.dumps(msg) + "\n").encode())
        f.flush()
        resp = f.readline().decode()
        print(f"{name:7} rtt={round((time.monotonic() - t) * 1000)}ms {resp.strip()}")


if __name__ == "__main__":
    main()
