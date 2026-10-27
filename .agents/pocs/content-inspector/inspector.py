#!/usr/bin/env python3
"""PROTOTYPE content inspector: egress classification protocol draft 0.2 over stdio, unix, or https."""

import argparse
import asyncio
import base64
import ipaddress
import json
import math
import os
import re
import sys
import threading
import time
from urllib.parse import parse_qsl, urlsplit

PROTOCOL_V = "0.2"
SUPPORTED = ("PII", "SECRET")
BODY_CAP = 1 << 20  # advertised as subjects.http.max_body_bytes
LINE_LIMIT = 16 << 20
HTTP_STATUS = {
    "invalid_request": 400,
    "unsupported_version": 400,
    "unsupported_subject": 400,
    "unauthorized": 401,
    "too_large": 413,
    "rate_limited": 429,
    "internal": 500,
}


def log(*a):
    print("[inspector]", *a, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- input


def segments(req, raw=None):
    """(where, text) pairs: URL query values, header values, body."""
    out = []
    url = req.get("url") or ""
    try:
        q = urlsplit(url).query
        for k, v in parse_qsl(q, keep_blank_values=True):
            out.append(("query", f"{k}={v}"))
    except ValueError:
        out.append(("url", url))
    for h in req.get("headers") or []:
        if isinstance(h, (list, tuple)) and len(h) == 2:
            out.append(("header", f"{h[0]}: {h[1]}"))
    if raw:
        out.append(("body", raw.decode("utf-8", errors="replace")))
    return out


# ---------------------------------------------------------------- masking


def mask_tail(s, keep=4):
    return "*" * max(len(s) - keep, 0) + s[-keep:]


def mask_head(s, keep=4):
    return s[:keep] + "*" * max(len(s) - keep, 0)


def mask_email(s):
    user, _, dom = s.partition("@")
    return f"{user[:1]}***@{dom}"


# ---------------------------------------------------------------- regex backend

RE_EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
RE_SSN = re.compile(r"(?<!\d)(?!000|666|9\d\d)\d{3}-(?!00)\d{2}-(?!0000)\d{4}(?!\d)")
RE_PHONE = re.compile(r"(?<![\d-])(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]\d{3}[-.\s]\d{4}(?![\d-])")
RE_CARD = re.compile(r"(?<!\d)(?:\d[ -]?){12,18}\d(?!\d)")
RE_AWS = re.compile(r"(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])")
RE_GH = re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})\b")
RE_PEM = re.compile(r"-----BEGIN ((?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?PRIVATE KEY)-----")
RE_GENERIC = re.compile(
    r"(?i)\b([a-z0-9_.-]*(?:api[_-]?key|secret|token|passw(?:or)?d|access[_-]?key)[a-z0-9_.-]*)"
    r"[\"']?\s*[:=]\s*[\"']?([A-Za-z0-9_\-/+=.]{16,})"
)


def luhn(digits):
    total, alt = 0, False
    for c in reversed(digits):
        d = int(c)
        if alt:
            d *= 2
            if d > 9:
                d -= 9
        total += d
        alt = not alt
    return total % 10 == 0


def entropy(s):
    n = len(s)
    return -sum(s.count(c) / n * math.log2(s.count(c) / n) for c in set(s))


def regex_pii(text):
    for m in RE_EMAIL.finditer(text):
        yield ("email", 0.95, mask_email(m.group()))
    for m in RE_SSN.finditer(text):
        yield ("us_ssn", 0.9, "***-**-" + m.group()[-4:])
    for m in RE_CARD.finditer(text):
        d = re.sub(r"\D", "", m.group())
        if 13 <= len(d) <= 19 and luhn(d):
            yield ("credit_card", 0.9, mask_tail(d))
    for m in RE_PHONE.finditer(text):
        d = re.sub(r"\D", "", m.group())
        yield ("phone", 0.6, mask_tail(d))


def regex_secret(text):
    for m in RE_AWS.finditer(text):
        yield ("aws_access_key_id", 0.99, mask_head(m.group()))
    for m in RE_GH.finditer(text):
        yield ("github_token", 0.99, mask_head(m.group(), 4))
    for m in RE_PEM.finditer(text):
        yield ("private_key", 0.99, f"-----BEGIN {m.group(1)}-----")
    for m in RE_GENERIC.finditer(text):
        val = m.group(2)
        if entropy(val) >= 3.5:
            yield ("generic_api_key", 0.7, f"{m.group(1)}=***")


class RegexBackend:
    name = "regex"
    labels = ("PII", "SECRET")

    def classify(self, segs, labels):
        out = []
        for _, text in segs:
            if "PII" in labels:
                out += [("PII", *f) for f in regex_pii(text)]
            if "SECRET" in labels:
                out += [("SECRET", *f) for f in regex_secret(text)]
        return out


# ---------------------------------------------------------------- decider backend

# Customer-provided classifier: one yes/no question per label.
DECIDER_CLASSIFIER = {
    "PII": (
        "Does this outbound HTTP request contain personal identifying information about a "
        "person, such as an email address, phone number, social security number, credit card "
        "number, postal address, or full name tied to other personal data?",
        {
            "true": "the request carries personal data about an individual",
            "false": "the request carries no personal data; only code, config, or generic text",
        },
    ),
    "SECRET": (
        "Does this outbound HTTP request contain a secret credential, such as an API key, "
        "access key, password, bearer token, or private key?",
        {
            "true": "the request leaks a credential or key material",
            "false": "the request contains no credential or key material",
        },
    ),
}


class DeciderBackend:
    name = "decider"
    labels = ("PII", "SECRET")

    def __init__(self, checkpoint, device, threshold, max_state_chars):
        from strands_decider.infer import load_engine
        from strands_decider.schema import NoulQuestion

        t = time.monotonic()
        self.engine = load_engine(checkpoint, device=device)
        self.questions = {
            label: NoulQuestion(instructions=q, criteria=c)
            for label, (q, c) in DECIDER_CLASSIFIER.items()
        }
        self.threshold = threshold
        self.max_state_chars = max_state_chars
        self.lock = threading.Lock()
        log(f"decider loaded {checkpoint} on {device} in {time.monotonic() - t:.1f}s")
        self.classify([("body", "warmup")], list(SUPPORTED))
        log("decider warm")

    def classify(self, segs, labels):
        state = "\n".join(f"[{w}] {t}" for w, t in segs)[: self.max_state_chars]
        qs = {lab: self.questions[lab] for lab in labels if lab in self.questions}
        if not qs:
            return []
        with self.lock:
            resp = self.engine.ask(state, qs)
        out = []
        for lab, ans in resp.answers.items():
            p = float(ans.noul)
            if p >= self.threshold:
                out.append((lab, "model", round(p, 3), f"decider p(true)={p:.3f}"))
        return out


# ---------------------------------------------------------------- presidio backend

PRESIDIO_KIND = {
    "EMAIL_ADDRESS": "email",
    "PHONE_NUMBER": "phone",
    "US_SSN": "us_ssn",
    "CREDIT_CARD": "credit_card",
    "IBAN_CODE": "iban",
    "IP_ADDRESS": "ip_address",
    "PERSON": "person",
    "US_PASSPORT": "us_passport",
    "US_DRIVER_LICENSE": "us_driver_license",
}


class PresidioBackend:
    """PII only; SECRET is not a Presidio concept and is reported as unsupported."""

    name = "presidio"
    labels = ("PII",)

    def __init__(self):
        from presidio_analyzer import AnalyzerEngine
        from presidio_analyzer.nlp_engine import NlpEngineProvider

        nlp = NlpEngineProvider(
            nlp_configuration={
                "nlp_engine_name": "spacy",
                "models": [{"lang_code": "en", "model_name": "en_core_web_sm"}],
            }
        ).create_engine()
        self.analyzer = AnalyzerEngine(nlp_engine=nlp, supported_languages=["en"])
        self.lock = threading.Lock()
        self.analyzer.analyze(text="warmup", language="en")
        log("presidio warm")

    def classify(self, segs, labels):
        if "PII" not in labels:
            return []
        out = []
        for _, text in segs:
            with self.lock:
                res = self.analyzer.analyze(
                    text=text, language="en", entities=list(PRESIDIO_KIND)
                )
            for r in res:
                if r.score < 0.5:
                    continue
                val = text[r.start : r.end]
                kind = PRESIDIO_KIND[r.entity_type]
                ev = mask_email(val) if kind == "email" else mask_tail(val, 2)
                out.append(("PII", kind, round(r.score, 3), ev))
        return out


# ---------------------------------------------------------------- dns and connect (backend-independent)

DNS_LABELS = ("MALICIOUS_DOMAIN", "TUNNELING")
CONNECT_LABELS = ("MALICIOUS_IP",)
NAME_CACHE_TTL_S = 300
BUILTIN_DOMAINS = {"malware.testing.google.test", "evil.example", "phishing.example", "c2.example"}
RE_HEXISH = re.compile(r"^(?:[0-9a-f]{16,}|[a-z2-7]{24,})$")


class Blocklist:
    """One entry per line; `#` starts a comment. Re-read when the file changes."""

    def __init__(self, path, parse):
        self.path, self.parse, self.stamp, self.entries = path, parse, None, []
        self.lock = threading.Lock()

    def get(self):
        if not self.path:
            return []
        with self.lock:
            try:
                st = os.stat(self.path)
                stamp = (st.st_mtime_ns, st.st_size)
            except OSError:
                stamp = None
            if stamp != self.stamp:
                self.stamp, self.entries = stamp, []
                if stamp is not None:
                    with open(self.path) as f:
                        for line in f:
                            line = line.split("#", 1)[0].strip()
                            if line:
                                self.entries.append(self.parse(line))
                log(f"loaded {len(self.entries)} entries from {self.path}")
            return self.entries


DOMAINS = Blocklist(None, lambda l: l.lower().rstrip("."))
NETWORKS = Blocklist(None, lambda l: ipaddress.ip_network(l, strict=False))


def tunneling(name):
    labels = name.split(".")
    if len(name) > 100:
        return ("long_name", 0.8, f"{len(name)} characters")
    for lab in labels:
        if len(lab) >= 30 and entropy(lab) >= 3.5:
            return ("high_entropy_label", 0.9, f"{len(lab)}-char label, entropy {entropy(lab):.2f}")
    encoded = [lab for lab in labels[:-2] if RE_HEXISH.match(lab)]
    if len(encoded) >= 2:
        return ("encoded_labels", 0.7, f"{len(encoded)} hex/base32 labels")
    return None


def malicious_domain(name):
    parts = name.split(".")
    listed = BUILTIN_DOMAINS | set(DOMAINS.get())
    for i in range(len(parts)):
        if ".".join(parts[i:]) in listed:
            return ("blocklist", 1.0, ".".join(parts[i:]))
    return None


def malicious_ip(ip):
    try:
        addr = ipaddress.ip_address(ip)
    except ValueError:
        raise ProtocolError("invalid_request", f"subject.ip {ip!r} is not an address")
    for net in NETWORKS.get():
        if addr.version == net.version and addr in net:
            return ("blocklist", 1.0, str(net))
    return None


def classify_name(subject, labels):
    name = subject.get("name")
    if not isinstance(name, str) or not name:
        raise ProtocolError("invalid_request", "subject.name is required")
    out = []
    if subject["kind"] == "dns":
        if "TUNNELING" in labels and (f := tunneling(name)):
            out.append(("TUNNELING", *f))
        if "MALICIOUS_DOMAIN" in labels and (f := malicious_domain(name)):
            out.append(("MALICIOUS_DOMAIN", *f))
    else:
        if "MALICIOUS_IP" in labels and (f := malicious_ip(subject.get("ip") or "")):
            out.append(("MALICIOUS_IP", *f))
    return out


# ---------------------------------------------------------------- protocol


class ProtocolError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def dedupe(findings):
    seen, out = set(), []
    for label, kind, conf, ev in findings:
        k = (label, kind, ev)
        if k not in seen:
            seen.add(k)
            out.append({"label": label, "kind": kind, "confidence": conf, "evidence": ev})
    return out


def capabilities(backend):
    return {
        "versions": [PROTOCOL_V],
        "inspector": {"name": backend.name, "version": "0.2-prototype"},
        "subjects": {
            "http": {"labels": list(backend.labels), "max_body_bytes": BODY_CAP},
            "dns": {"labels": list(DNS_LABELS)},
            "connect": {"labels": list(CONNECT_LABELS)},
        },
    }


def classify(backend, params, rid):
    subject = params.get("subject")
    if not isinstance(subject, dict):
        raise ProtocolError("invalid_request", "params.subject is required")
    kind = subject.get("kind")
    if kind not in ("http", "dns", "connect"):
        raise ProtocolError("unsupported_subject", f"this inspector does not classify {kind!r}")
    asked = params.get("labels")
    if not isinstance(asked, list) or not asked or not all(isinstance(l, str) for l in asked):
        raise ProtocolError("invalid_request", "params.labels must be a non-empty list of strings")
    if kind != "http":
        supported = DNS_LABELS if kind == "dns" else CONNECT_LABELS
        labels = [l for l in dict.fromkeys(asked) if l in supported]
        findings = dedupe(classify_name(subject, labels))
        found = [l for l in dict.fromkeys(f["label"] for f in findings) if l in labels]
        where = subject.get("name") if kind == "dns" else f"{subject.get('name')} {subject.get('ip')}"
        log(f"{rid} classify {kind} {where}:{subject.get('port')} asked={asked} -> {found}")
        return {"labels": found, "findings": findings, "cache_ttl_s": NAME_CACHE_TTL_S}
    raw = b""
    if subject.get("body_b64"):
        try:
            raw = base64.b64decode(subject["body_b64"], validate=True)
        except ValueError:
            raise ProtocolError("invalid_request", "body_b64 is not standard base64")
    if len(raw) > BODY_CAP:
        raise ProtocolError("too_large", f"body is {len(raw)} bytes, limit is {BODY_CAP}")
    labels = [l for l in dict.fromkeys(asked) if l in backend.labels]
    segs = segments(subject, raw)
    t = time.monotonic()
    findings = dedupe(backend.classify(segs, labels)) if labels and segs else []
    ms = round((time.monotonic() - t) * 1000)
    found = [l for l in dict.fromkeys(f["label"] for f in findings) if l in labels]
    log(
        f"{rid} classify {subject.get('method')} {subject.get('url')} "
        f"headers={subject.get('headers')} asked={asked} -> {found} {ms}ms"
    )
    return {"labels": found, "findings": [f for f in findings if f["label"] in found]}


def respond(backend, line):
    """One request line (bytes or str) -> one response dict. Never raises."""
    rid = None
    try:
        try:
            msg = json.loads(line)
        except ValueError:
            raise ProtocolError("invalid_request", "the message is not JSON")
        if not isinstance(msg, dict):
            raise ProtocolError("invalid_request", "the message is not an object")
        rid = msg.get("id") if isinstance(msg.get("id"), str) else None
        if rid is None:
            raise ProtocolError("invalid_request", "missing string id")
        v = msg.get("v")
        if not isinstance(v, str) or v.split(".")[0] != PROTOCOL_V.split(".")[0]:
            raise ProtocolError("unsupported_version", f"unsupported protocol version {v!r}")
        op, params = msg.get("op"), msg.get("params")
        if not isinstance(params, dict):
            raise ProtocolError("invalid_request", "params must be an object")
        if op == "capabilities":
            result = capabilities(backend)
            log(f"{rid} capabilities")
        elif op == "classify":
            result = classify(backend, params, rid)
        else:
            raise ProtocolError("invalid_request", f"unknown op {op!r}")
        return {"v": PROTOCOL_V, "id": rid, "result": result}
    except ProtocolError as e:
        log(f"{rid} error {e.code}: {e}")
        return {"v": PROTOCOL_V, "id": rid, "error": {"code": e.code, "message": str(e)}}
    except Exception as e:  # noqa: BLE001 - prototype: every failure becomes a typed error
        log(f"{rid} error internal: {type(e).__name__}: {e}")
        return {"v": PROTOCOL_V, "id": rid, "error": {"code": "internal", "message": f"{type(e).__name__}: {e}"}}


# ---------------------------------------------------------------- line bindings (stdio, unix)


async def pump(backend, reader, emit):
    """Read one message per line, answer each concurrently; `emit(bytes)` is an async writer."""
    lock = asyncio.Lock()
    loop = asyncio.get_running_loop()

    async def one(line):
        resp = await loop.run_in_executor(None, respond, backend, line)
        async with lock:
            await emit((json.dumps(resp) + "\n").encode())

    tasks = set()
    while True:
        try:
            line = await reader.readline()
        except (ValueError, asyncio.LimitOverrunError):
            err = {"v": PROTOCOL_V, "id": None, "error": {"code": "invalid_request", "message": "line over 16 MiB"}}
            async with lock:
                await emit((json.dumps(err) + "\n").encode())
            break
        if not line:
            break
        if line.strip():
            t = asyncio.create_task(one(line))
            tasks.add(t)
            t.add_done_callback(tasks.discard)
    if tasks:
        await asyncio.gather(*tasks)


async def serve_conn(backend, reader, writer):
    async def emit(b):
        writer.write(b)
        await writer.drain()

    try:
        await pump(backend, reader, emit)
    except ConnectionError:
        pass
    finally:
        writer.close()


async def serve_stdio(backend, out):
    loop = asyncio.get_running_loop()
    reader = asyncio.StreamReader(limit=LINE_LIMIT)
    await loop.connect_read_pipe(lambda: asyncio.StreamReaderProtocol(reader), sys.stdin)

    async def emit(b):
        out.write(b)
        out.flush()

    log(f"serving stdio backend={backend.name}")
    await pump(backend, reader, emit)


# ---------------------------------------------------------------- https binding


def serve_https(backend, addr, cert, key, token):
    import http.server
    import ssl

    host, _, port = addr.rpartition(":")
    if host not in ("127.0.0.1", "localhost"):
        raise SystemExit("--https binds 127.0.0.1 only")

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):
            log("https", fmt % args)

        def reply(self, resp):
            body = json.dumps(resp).encode()
            err = resp.get("error")
            status = HTTP_STATUS.get(err["code"], 500) if err else 200
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            if length > LINE_LIMIT:
                self.close_connection = True
                return self.reply({"v": PROTOCOL_V, "id": None, "error": {"code": "invalid_request", "message": "message over 16 MiB"}})
            raw = self.rfile.read(length)
            if token is not None and self.headers.get("Authorization") != f"Bearer {token}":
                try:
                    rid = json.loads(raw).get("id")
                except Exception:  # noqa: BLE001
                    rid = None
                rid = rid if isinstance(rid, str) else None
                log(f"{rid} error unauthorized: bad or missing bearer token")
                return self.reply({"v": PROTOCOL_V, "id": rid, "error": {"code": "unauthorized", "message": "bad or missing bearer token"}})
            self.reply(respond(backend, raw))

    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    ctx.load_cert_chain(cert, key)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", int(port)), Handler)
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
    log(f"listening on https://127.0.0.1:{port} backend={backend.name} token={'set' if token else 'none'}")
    server.serve_forever()


def build_backend(args):
    if args.backend == "regex":
        return RegexBackend()
    if args.backend == "decider":
        return DeciderBackend(args.decider_model, args.device, args.threshold, args.max_state_chars)
    if args.backend == "presidio":
        return PresidioBackend()
    raise SystemExit(f"unknown backend {args.backend}")


async def main():
    ap = argparse.ArgumentParser()
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--stdio", action="store_true", help="one message per line on stdin/stdout")
    mode.add_argument("--socket", default=os.environ.get("INSPECTOR_SOCKET"), help="unix socket path")
    mode.add_argument("--https", metavar="HOST:PORT", help="https on 127.0.0.1 only")
    ap.add_argument("--cert")
    ap.add_argument("--key")
    ap.add_argument("--token", default=os.environ.get("INSPECTOR_TOKEN"))
    ap.add_argument("--backend", choices=["regex", "decider", "presidio"], default="regex")
    ap.add_argument("--decider-model", default="StrandsAgents/strands-decider-2B-hobson-v19")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--threshold", type=float, default=0.5)
    ap.add_argument("--max-state-chars", type=int, default=16384)
    ap.add_argument("--domain-blocklist", help="MALICIOUS_DOMAIN names, one per line; re-read on change")
    ap.add_argument("--ip-blocklist", help="MALICIOUS_IP addresses or CIDRs, one per line; re-read on change")
    args = ap.parse_args()
    DOMAINS.path, NETWORKS.path = args.domain_blocklist, args.ip_blocklist
    if args.stdio:
        # Keep the real stdout for protocol lines only; anything else printing to fd 1
        # (libraries, C extensions) lands on stderr.
        out = os.fdopen(os.dup(1), "wb", buffering=0)
        os.dup2(2, 1)
        sys.stdout = sys.stderr
        backend = build_backend(args)
        await serve_stdio(backend, out)
        return
    if args.https:
        if not (args.cert and args.key):
            raise SystemExit("--https needs --cert and --key")
        backend = build_backend(args)
        await asyncio.get_running_loop().run_in_executor(
            None, serve_https, backend, args.https, args.cert, args.key, args.token
        )
        return
    if not args.socket:
        raise SystemExit("one of --stdio, --socket PATH, or --https HOST:PORT is required")
    backend = build_backend(args)
    if os.path.exists(args.socket):
        os.unlink(args.socket)
    old = os.umask(0o177)
    try:
        server = await asyncio.start_unix_server(
            lambda r, w: serve_conn(backend, r, w), path=args.socket, limit=LINE_LIMIT
        )
    finally:
        os.umask(old)
    os.chmod(args.socket, 0o600)
    log(f"listening on unix {args.socket} backend={backend.name}")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
