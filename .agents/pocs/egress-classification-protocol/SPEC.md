# PROTOTYPE — Egress Classification Protocol, draft 0.2

Status: draft for discussion. Not stable. Not implemented in full. Section 11 lists what the
prototype on this branch implements.

## 1. Purpose

An agent sandbox sends outbound traffic through a gateway. The operator wants to stop some of that
traffic because of what it is or what it carries: personal data in a request body, a credential, a
malicious domain, a tunnelling pattern.

This protocol lets any gateway ask any classifier one question: **what is this?** The classifier
answers with labels. The gateway's own policy decides what to do with the labels.

The protocol has two roles:

| Role | Does | Does not |
|---|---|---|
| **Gateway** | Builds the subject, calls the inspector, gives the labels to its policy, enforces the decision. | Classify. |
| **Inspector** | Classifies the subject and returns labels. | Decide. It returns no allow, deny, or modify verdict. |

The rule in the second row is the reason this protocol exists. A protocol that returns verdicts (for
example ICAP) moves the policy into the vendor. This protocol keeps the policy in the gateway.

The key words MUST, MUST NOT, SHOULD, and MAY are as in RFC 2119.

## 2. Messages

Every message is one JSON object, UTF-8. A message carries its own operation, its own
correlation, and its own error. The transport carries bytes only, so the same messages work over
every binding in section 3.

### 2.1 Request

```json
{ "v": "0.2", "id": "box-42", "op": "classify", "params": { } }
```

| Field | Required | Meaning |
|---|---|---|
| `v` | yes | The protocol version of this message. |
| `id` | yes | Gateway-chosen string. It MUST be unique among the requests in flight on one connection. |
| `op` | yes | `capabilities` or `classify`. |
| `params` | yes | The operation's input. `{}` for `capabilities`. |

### 2.2 Response

A response has the request's `id` and exactly one of `result` or `error`:

```json
{ "v": "0.2", "id": "box-42", "result": { } }
{ "v": "0.2", "id": "box-42", "error": { "code": "too_large", "message": "body is 3 MiB, limit is 1 MiB" } }
```

- An inspector MAY answer requests in any order. The gateway pairs a response with its request by
  `id`, never by position.
- A gateway MAY have more than one request in flight on one connection.
- When the inspector cannot read a request's `id`, it answers with `"id": null` and an
  `invalid_request` error.
- A message MUST NOT be larger than 16 MiB.

### 2.3 Versions

- `v` has the form `MAJOR.MINOR`. A change of `MINOR` only adds optional fields. A receiver MUST
  ignore a field it does not know.
- An inspector that does not support the request's `MAJOR` answers with `unsupported_version`.
- In this draft, `MAJOR` is `0`, so every change can break. A `1.0` freezes the major version.

## 3. Bindings

A binding says how the bytes of one message travel, and how each side knows who the other is.
The message is the same in every binding. Only these three bindings are defined:

| Binding | Endpoint | Framing | Peer identity |
|---|---|---|---|
| **stdio** | the gateway starts a command | one message per line | the gateway started the process |
| **unix** | `unix:///var/run/inspector.sock` | one message per line | the socket's owner |
| **https** | `https://inspect.vendor.example/v1/classify` | one message per HTTP body | TLS, and a bearer token |

### 3.1 stdio

- The gateway starts the inspector and writes requests to its standard input. The inspector writes
  responses to its standard output.
- Each message is one line, ended by `\n`. JSON escapes every newline inside a string, so a message
  never holds a raw newline.
- Standard output carries messages only. Logs go to standard error. A gateway MAY show standard
  error to the operator.
- When standard input closes, the inspector finishes the requests in flight and exits.
- The inspector is long-lived. One process serves every request for its lifetime.

### 3.2 unix

- A stream socket at the path in the endpoint URI. Framing is the same as stdio: one message per
  line. One connection MAY carry many requests.
- Before it sends a subject, the gateway MUST check that the socket is owned by the expected user
  and is not writable by other users.

### 3.3 https

- The gateway sends each request as the body of one `POST` to the endpoint URI, with
  `Content-Type: application/json`. The response body is one response message.
- TLS is REQUIRED. Plain `http://` MUST NOT be used, because a subject can hold decrypted payload
  data.
- The gateway sends `Authorization: Bearer <token>`. The operator supplies the token through the
  gateway's credential system. Mutual TLS MAY be used in addition.
- The HTTP status mirrors the result, so that load balancers, gateways, and monitoring see errors:
  `200` with `result`, and the status in section 7 with `error`. **The body is the source of
  truth.** When the status and the body disagree, the gateway uses the body. A response without a
  valid message body is a failed decision.

## 4. Operations

### 4.1 `capabilities`

The gateway sends this request when it starts, and after each error that section 7 marks as
"refresh". The gateway MUST refuse to use an inspector whose major version it does not support.

Request:

```json
{ "v": "0.2", "id": "box-1", "op": "capabilities", "params": {} }
```

Response:

```json
{
  "v": "0.2",
  "id": "box-1",
  "result": {
    "versions": ["0.2"],
    "inspector": { "name": "acme-dlp", "version": "4.2.0" },
    "subjects": {
      "http":    { "labels": ["PII", "SECRET", "acme:source_code"], "max_body_bytes": 1048576 },
      "dns":     { "labels": ["MALICIOUS_DOMAIN", "TUNNELING", "NEWLY_REGISTERED"] },
      "connect": { "labels": ["MALICIOUS_IP"] }
    }
  }
}
```

- An inspector lists only the subject kinds it supports. An inspector that classifies only names
  omits `http` and `connect`.
- `max_body_bytes` is the largest body the inspector accepts. The gateway MUST NOT send a larger
  body. Section 8 says what the gateway does instead.

### 4.2 `classify`

Request:

```json
{
  "v": "0.2",
  "id": "box-42",
  "op": "classify",
  "params": {
    "subject": { "kind": "dns", "name": "xn--bcher-kva.example", "port": 443 },
    "labels": ["MALICIOUS_DOMAIN", "TUNNELING"],
    "context": { "gateway": "strands-box", "box": "my-agent", "caller": "agent" }
  }
}
```

| Field in `params` | Required | Meaning |
|---|---|---|
| `subject` | yes | The thing to classify. Section 5. |
| `labels` | yes | The labels the policy asks about. Not empty. |
| `context` | no | Who asked. Opaque to the protocol. An inspector MAY use it for per-tenant rules and audit. |

Response:

```json
{
  "v": "0.2",
  "id": "box-42",
  "result": {
    "labels": ["TUNNELING"],
    "findings": [
      { "label": "TUNNELING", "kind": "high_entropy_label", "confidence": 0.91 }
    ],
    "cache_ttl_s": 300
  }
}
```

| Field in `result` | Required | Meaning |
|---|---|---|
| `labels` | yes | The requested labels the inspector found. A subset of the request `labels`. Empty means none found. |
| `findings` | no | Detail for audit. Section 6.2. |
| `cache_ttl_s` | no | How many seconds the gateway MAY reuse this answer for the same subject and labels. Section 5 says which subjects allow it. Absent or `0` means do not reuse. |

- The inspector MUST NOT put a label in `labels` that the request did not ask for.
- A requested label that the inspector does not support for that subject kind is not found. The
  inspector does not report it. The gateway is responsible for not asking (section 8).

## 5. Subjects

Each subject has a `kind`. The gateway sends one subject per request.

### 5.1 `dns` — a name, before it is resolved

```json
{ "kind": "dns", "name": "api.github.com", "port": 443 }
```

- `name` is the host name the gateway will resolve. Lower case. No trailing dot. An
  internationalized name is in its ASCII (`xn--`) form.
- `port` is the destination port the caller asked for. It is a hint. DNS does not use it.
- The gateway sends this subject **before** any DNS query leaves. A name that the policy refuses
  is never resolved.
- `cache_ttl_s` MAY be used.

### 5.2 `connect` — an address, before the socket opens

```json
{ "kind": "connect", "name": "api.github.com", "ip": "140.82.112.6", "port": 443 }
```

- `ip` is the resolved address the gateway will connect to. One request per address.
- `cache_ttl_s` MAY be used.

### 5.3 `http` — a request, after TLS interception

```json
{
  "kind": "http",
  "method": "POST",
  "url": "https://api.github.com/repos/acme/app/issues",
  "headers": [["content-type", "application/json"], ["authorization", "[redacted]"]],
  "body_b64": "eyJ0aXRsZSI6ImJ1ZyIsImJvZHkiOiJTU04gMTIzLTQ1LTY3ODkifQ=="
}
```

- `headers` is an ordered list of `[name, value]` pairs. Names are lower case. A repeated header is
  repeated in the list.
- The gateway MUST replace the value of `authorization`, `proxy-authorization`, and `cookie` with
  `[redacted]`. The gateway MUST send the headers that the workload sent, before the gateway adds
  any credential of its own.
- `body_b64` is the complete request body in standard base64. Absent means no body.
- `cache_ttl_s` MUST NOT be used. The gateway ignores it for this subject.

A later version adds `http_response` for response bodies.

## 6. Labels and findings

### 6.1 Label names

- A **base label** is upper case: `PII`. This specification defines the base labels.
- A **vendor label** has a vendor prefix and a colon: `acme:source_code`. The vendor defines it.
- A policy that uses only base labels works with every inspector that supports them.

Base labels in 0.2:

| Label | Subject kinds | Meaning |
|---|---|---|
| `PII` | `http` | Personal data about an identifiable person. |
| `SECRET` | `http` | A credential, key, or token. |
| `MALICIOUS_DOMAIN` | `dns` | The name is known to serve malware, phishing, or command and control. |
| `NEWLY_REGISTERED` | `dns` | The registrable domain is recent. The inspector defines "recent". |
| `TUNNELING` | `dns` | The name has the shape of data encoded into DNS labels. |
| `MALICIOUS_IP` | `connect` | The address is known to serve malware, phishing, or command and control. |

### 6.2 Findings

```json
{ "label": "PII", "kind": "us_ssn", "confidence": 0.97, "evidence": "***-**-6789",
  "location": { "part": "body", "start": 34, "end": 45 } }
```

| Field | Required | Meaning |
|---|---|---|
| `label` | yes | One of the response `labels`. |
| `kind` | no | Inspector-defined detail, for example `email`, `us_ssn`, `aws_access_key`, `model`. |
| `confidence` | no | 0.0 to 1.0. |
| `evidence` | no | A masked hint. It MUST NOT contain the sensitive value. |
| `location` | no | `part` is `url`, `header`, or `body`. `start` and `end` are byte offsets in that part. |

Findings are for audit. A gateway policy decides on `labels`, not on findings.

## 7. Errors

An error is a response with an `error` object in place of `result`:

```json
{ "v": "0.2", "id": "box-42", "error": { "code": "unsupported_subject", "message": "this inspector does not classify connect" } }
```

`code` is one of the values below. `message` is free text for logs. The https binding mirrors the
code into the HTTP status.

| `code` | https status | Gateway action |
|---|---|---|
| `invalid_request` | 400 | Fail the decision. |
| `unsupported_version` | 400 | Fail the decision. Refresh capabilities. |
| `unsupported_subject` | 400 | Fail the decision. Refresh capabilities. |
| `unauthorized` | 401 or 403 | Fail the decision. |
| `too_large` | 413 | Fail the decision. Refresh capabilities. |
| `rate_limited` | 429 | Fail the decision. The gateway MAY honour `Retry-After` for later requests. |
| `internal` | 500 | Fail the decision. |

A code the gateway does not know is treated as `internal`. "Fail the decision" is defined in
section 8.

## 8. Gateway obligations

1. **Fail closed.** When the inspector returns an error, does not answer before the deadline, is
   unreachable, or returns a response that does not match this specification, the gateway MUST
   treat the decision as failed. A failed decision MUST NOT allow the effect. There is no
   configuration that makes a failed decision allow.
2. **Deadline.** The gateway MUST apply an end-to-end deadline to each classify call.
3. **Body limit.** When a body is larger than `max_body_bytes`, the gateway MUST NOT truncate it and
   MUST NOT send it. The gateway treats the decision as failed.
4. **Ask only what is supported.** The gateway sends a request only with labels that the
   inspector's capabilities list for that subject kind.
5. **More than one inspector.** When more than one inspector supports a requested label, the gateway
   asks each one and takes the union of the `labels`. A label that any inspector finds is found.
6. **Only when needed.** The gateway SHOULD call an inspector only when the policy outcome can
   depend on the answer.
7. **Cache.** The gateway MAY reuse a `dns` or `connect` answer for `cache_ttl_s` seconds, keyed on
   the subject and the requested labels. It MUST NOT reuse an `http` answer.
8. **Workload isolation.** The workload MUST NOT be able to reach the inspector. Otherwise it can
   use the inspector to test which content passes.

## 9. Inspector obligations

1. **No verdicts.** The inspector returns labels. It MUST NOT return allow, deny, or a changed
   message.
2. **No retention.** The inspector MUST NOT store a subject after it sends the response, except
   where the operator has an agreement with the vendor that says otherwise.
3. **No raw evidence.** `evidence` MUST NOT contain the sensitive value.
4. **Stateless calls.** The answer to a request MUST NOT depend on an earlier request from the same
   gateway. Patterns across requests belong to the gateway's policy, which has the history.

## 10. Security considerations

- **Payload exposure.** An `http` subject holds decrypted data, which is often the same data the
  operator wants to keep in. The operator trusts the inspector with that data. A remote inspector
  sends it to the vendor.
- **Credentials.** Section 5.3 removes the credentials that the gateway knows about. A credential
  in a body or a URL is still sent. Detecting it is the inspector's job (`SECRET`).
- **Availability.** Fail-closed makes the inspector part of the egress path. An inspector outage
  stops all classified traffic. That is intended.
- **Evasion.** An inspector that reads only part of a body misses what it does not read. Section 8
  item 3 makes the gateway refuse bodies the inspector cannot read in full.
- **Cache poisoning.** A cached `dns` answer is keyed on the exact normalized name, so one name
  cannot set the answer for another.

## 11. Strands Box mapping and prototype status

### Configuration (proposed)

```toml
[egress.inspector]
endpoint   = "https://inspect.vendor.example/v1/classify"   # https binding
secret.ref = "env://INSPECTOR_TOKEN"                           # a Box credential locator
timeout_ms = 2000
```

```toml
[egress.inspector]
endpoint = "unix:///var/run/inspector.sock"                  # unix binding, inspector outside the box
```

```toml
[egress.inspector]
command = ["/path/to/inspector", "--stdio"]                   # stdio binding, Box starts a contained leaf
gpu     = true                                                # macOS: Metal for this leaf only
[egress.inspector.filesystem]
read = ["/path/to/inspector"]
```

Exactly one of `endpoint` and `command` is set.

### Policy (proposed)

| Box action | Subject | Provider |
|---|---|---|
| `net:resolve` (proposed; today the first `net:connect`, without `ip`) | `dns` | `Inspection::Domain([...])` |
| `net:connect` | `connect` | `Inspection::Address([...])` |
| `http:request` | `http` | `Inspection::Content([...])` |

```cedar
forbid (principal, action == Box::Action::"http:request", resource)
when { Inspection::Content(["PII", "SECRET"]).labels.containsAny(["PII", "SECRET"]) };

forbid (principal, action == Box::Action::"net:resolve", resource)
when { Inspection::Domain(["MALICIOUS_DOMAIN", "TUNNELING"]).labels.containsAny(["MALICIOUS_DOMAIN", "TUNNELING"]) };
```

### What the prototype implements today

| This specification | Prototype on this branch |
|---|---|
| Message envelope (`v`, `id`, `op`, `result`, `error`) | Implemented. A response is paired by `id`. An unknown error code, a malformed response, and a label that was not requested fail the decision |
| stdio binding | Implemented, to a contained leaf. One process for the run, multiplexed by `id` |
| unix binding | Implemented. One long-lived connection, multiplexed by `id`, reconnected after it drops. Box checks that the socket is owned by the current user and is not group- or other-writable |
| https binding | Implemented. One `POST` per message. The body is the source of truth over the status. `http://` is refused at configuration |
| `capabilities` | Implemented, at startup only. Box refuses to start without a `0.x` version or the `http` subject. No refresh after an error |
| `dns` and `connect` subjects | Not implemented. `http` only |
| `Inspection::Domain`, `Inspection::Address`, `net:resolve` | Not implemented |
| `[egress.inspector] endpoint`, `command`, `secret.ref` | Implemented. `secret.ref` is a Box credential locator, for example `env://INSPECTOR_TOKEN`. A prototype `ca_file` adds a trust root for https |
| Fail closed, deadline, header redaction, body limit | Implemented. `timeout_ms` is an end-to-end deadline per call. The body limit is `subjects.http.max_body_bytes` |
| Ask only what is supported (8.4) | Implemented. Box sends only the labels the inspector lists for `http`, and warns once at startup about the others |
| More than one inspector (8.5) | Not implemented. One inspector per box |
| `cache_ttl_s` | Not implemented |
