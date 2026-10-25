#!/usr/bin/env python3
"""PROTOTYPE content inspector: NDJSON over a unix socket, pluggable backends."""

import argparse
import asyncio
import base64
import json
import math
import os
import re
import sys
import threading
import time
from urllib.parse import parse_qsl, urlsplit

PROTOCOL_V = 1
SUPPORTED = ("PII", "SECRET")
BODY_CAP = 1 << 20
LINE_LIMIT = 16 << 20


def log(*a):
    print("[inspector]", *a, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- input


def segments(req):
    """(where, text) pairs: URL query values, header values, body (capped)."""
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
    b64 = req.get("body_b64")
    if b64:
        raw = base64.b64decode(b64, validate=False)[:BODY_CAP]
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


# ---------------------------------------------------------------- server


def dedupe(findings):
    seen, out = set(), []
    for label, kind, conf, ev in findings:
        k = (label, kind, ev)
        if k not in seen:
            seen.add(k)
            out.append({"label": label, "kind": kind, "confidence": conf, "evidence": ev})
    return out


async def handle_line(backend, line):
    rid = None
    try:
        msg = json.loads(line)
        rid = msg.get("id")
        if msg.get("v") != PROTOCOL_V:
            raise ValueError(f"unsupported protocol version {msg.get('v')!r}")
        if not isinstance(rid, str):
            raise ValueError("missing string id")
        labels = [l for l in dict.fromkeys(msg.get("labels") or []) if l in SUPPORTED]
        segs = segments(msg.get("request") or {})
        t = time.monotonic()
        raw = (
            await asyncio.get_running_loop().run_in_executor(
                None, backend.classify, segs, labels
            )
            if labels and segs
            else []
        )
        ms = round((time.monotonic() - t) * 1000)
        findings = dedupe(raw)
        found = list(dict.fromkeys(f["label"] for f in findings))
        return {
            "v": PROTOCOL_V,
            "id": rid,
            "labels": found,
            "findings": findings,
            "backend": backend.name,
            "latency_ms": ms,
        }
    except Exception as e:  # noqa: BLE001 - prototype: every failure becomes a typed error
        return {"v": PROTOCOL_V, "id": rid, "error": f"{type(e).__name__}: {e}"}


async def pump(backend, reader, emit):
    """Read NDJSON lines, answer each concurrently; `emit(bytes)` is an async writer."""
    lock = asyncio.Lock()

    async def one(line):
        resp = await handle_line(backend, line)
        async with lock:
            await emit((json.dumps(resp) + "\n").encode())

    tasks = set()
    while True:
        try:
            line = await reader.readline()
        except (ValueError, asyncio.LimitOverrunError):
            async with lock:
                await emit((json.dumps({"v": PROTOCOL_V, "id": None, "error": "line too long"}) + "\n").encode())
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
    ap.add_argument("--socket", default=os.environ.get("INSPECTOR_SOCKET"))
    ap.add_argument("--backend", choices=["regex", "decider", "presidio"], default="regex")
    ap.add_argument("--decider-model", default="StrandsAgents/strands-decider-2B-hobson-v19")
    ap.add_argument("--device", default="cpu")
    ap.add_argument("--threshold", type=float, default=0.5)
    ap.add_argument("--max-state-chars", type=int, default=16384)
    ap.add_argument("--stdio", action="store_true", help="NDJSON on stdin/stdout instead of a socket")
    args = ap.parse_args()
    if args.stdio:
        # Keep the real stdout for protocol lines only; anything else printing to fd 1
        # (libraries, C extensions) lands on stderr.
        out = os.fdopen(os.dup(1), "wb", buffering=0)
        os.dup2(2, 1)
        sys.stdout = sys.stderr
        backend = build_backend(args)
        await serve_stdio(backend, out)
        return
    if not args.socket:
        raise SystemExit("--socket or $INSPECTOR_SOCKET required")
    backend = build_backend(args)
    if os.path.exists(args.socket):
        os.unlink(args.socket)
    server = await asyncio.start_unix_server(
        lambda r, w: serve_conn(backend, r, w), path=args.socket, limit=LINE_LIMIT
    )
    os.chmod(args.socket, 0o600)
    log(f"listening on {args.socket} backend={backend.name}")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
