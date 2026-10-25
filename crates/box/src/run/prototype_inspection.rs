//! PROTOTYPE(content-inspection): the client for a local content inspector, over a socket or a leaf's stdio.

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use policy::{ContentInspector, InspectionRequest, InspectionVerdict};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

use crate::record::config::prototype_inspection::InspectionConfig;

// PROTOTYPE(content-inspection): header names whose values never reach the inspector.
const REDACTED_HEADERS: [&str; 3] = ["authorization", "proxy-authorization", "cookie"];

// PROTOTYPE(content-inspection): one queued stdio request and the channel its reply goes to.
type Pending = (
    String,
    String,
    std::sync::mpsc::Sender<Result<String, String>>,
);

// PROTOTYPE(content-inspection): where requests go.
enum Transport {
    Socket(PathBuf),
    Leaf(tokio::sync::mpsc::UnboundedSender<Pending>),
}

/// PROTOTYPE(content-inspection): the box's one content inspector.
pub(crate) struct PrototypeInspector {
    transport: OnceLock<Transport>,
    timeout: Duration,
    next_id: AtomicU64,
    // PROTOTYPE(content-inspection): the leaf child and what must outlive it, in mode B.
    leaf: Mutex<Option<LeafHold>>,
}

struct LeafHold {
    child: tokio::process::Child,
    _retention: crate::run::contain::supervise::StreamingLeafRetention,
}

impl Drop for LeafHold {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl PrototypeInspector {
    /// PROTOTYPE(content-inspection): the inspector the record declares; a socket inspector is ready at once.
    pub(crate) fn from_config(config: &InspectionConfig) -> Arc<Self> {
        let inspector = Self {
            transport: OnceLock::new(),
            timeout: Duration::from_millis(config.content.timeout_ms),
            next_id: AtomicU64::new(1),
            leaf: Mutex::new(None),
        };
        if let Some(socket) = &config.content.socket {
            let _ = inspector.transport.set(Transport::Socket(socket.clone()));
            eprintln!(
                "strands-box: inspection: content inspector on socket {}",
                socket.display()
            );
        }
        Arc::new(inspector)
    }

    /// PROTOTYPE(content-inspection): adopt the leaf's stdio, in mode B.
    pub(crate) fn adopt_leaf(&self, parts: crate::run::contain::supervise::StreamingLeafParts) {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Pending>();
        let mut stdin = parts.stdin;
        let mut lines = tokio::io::BufReader::new(parts.stdout).lines();
        tokio::spawn(async move {
            let mut waiting: HashMap<String, std::sync::mpsc::Sender<Result<String, String>>> =
                HashMap::new();
            loop {
                tokio::select! {
                    queued = receiver.recv() => {
                        let Some((id, line, reply)) = queued else { break };
                        if let Err(error) = stdin.write_all(line.as_bytes()).await {
                            let _ = reply.send(Err(format!("write to inspector: {error}")));
                            continue;
                        }
                        let _ = stdin.flush().await;
                        waiting.insert(id, reply);
                    }
                    read = lines.next_line() => {
                        match read {
                            Ok(Some(line)) => {
                                let id = serde_json::from_str::<serde_json::Value>(&line)
                                    .ok()
                                    .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_owned));
                                if let Some(reply) = id.and_then(|id| waiting.remove(&id)) {
                                    let _ = reply.send(Ok(line));
                                }
                            }
                            Ok(None) | Err(_) => {
                                eprintln!("strands-box: inspection: the inspector leaf closed its stdout");
                                for (_, reply) in waiting.drain() {
                                    let _ = reply.send(Err("inspector exited".to_string()));
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });
        if let Ok(mut leaf) = self.leaf.lock() {
            *leaf = Some(LeafHold {
                child: parts.child,
                _retention: parts.retention,
            });
        }
        let _ = self.transport.set(Transport::Leaf(sender));
        eprintln!(
            "strands-box: inspection: content inspector leaf started (pid {})",
            parts.leader
        );
    }

    fn exchange(&self, id: &str, line: String) -> Result<String, String> {
        match self.transport.get() {
            None => Err("the inspector is not started".to_string()),
            Some(Transport::Socket(path)) => {
                let stream = UnixStream::connect(path)
                    .map_err(|e| format!("connect {}: {e}", path.display()))?;
                stream
                    .set_read_timeout(Some(self.timeout))
                    .and_then(|()| stream.set_write_timeout(Some(self.timeout)))
                    .map_err(|e| e.to_string())?;
                (&stream)
                    .write_all(line.as_bytes())
                    .map_err(|e| format!("write: {e}"))?;
                let mut reader = BufReader::new(&stream);
                let mut reply = String::new();
                reader
                    .read_line(&mut reply)
                    .map_err(|e| format!("read: {e}"))?;
                if reply.is_empty() {
                    return Err("the inspector closed the connection".to_string());
                }
                Ok(reply)
            }
            Some(Transport::Leaf(sender)) => {
                let (reply, wait) = std::sync::mpsc::channel();
                sender
                    .send((id.to_string(), line, reply))
                    .map_err(|_| "the inspector leaf is gone".to_string())?;
                wait.recv_timeout(self.timeout)
                    .map_err(|_| format!("no reply within {} ms", self.timeout.as_millis()))?
            }
        }
    }
}

impl ContentInspector for PrototypeInspector {
    fn inspect(&self, request: &InspectionRequest<'_>) -> Result<InspectionVerdict, String> {
        let id = format!("box-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let headers: Vec<[String; 2]> = request
            .headers
            .iter()
            .map(|(name, value)| {
                let lower = name.to_ascii_lowercase();
                let value = if REDACTED_HEADERS.contains(&lower.as_str()) {
                    "[redacted]".to_string()
                } else {
                    value.clone()
                };
                [lower, value]
            })
            .collect();
        let message = serde_json::json!({
            "v": 1,
            "id": id,
            "labels": request.labels,
            "request": {
                "method": request.method,
                "url": request.url,
                "headers": headers,
                "body_b64": base64::engine::general_purpose::STANDARD.encode(request.body),
            }
        });
        let mut line = message.to_string();
        line.push('\n');
        let reply = self.exchange(&id, line)?;
        let reply: serde_json::Value =
            serde_json::from_str(reply.trim()).map_err(|e| format!("reply is not JSON: {e}"))?;
        if reply.get("v").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err("reply has no `v: 1`".to_string());
        }
        if reply.get("id").and_then(|v| v.as_str()) != Some(id.as_str()) {
            return Err("reply id does not match".to_string());
        }
        if let Some(error) = reply.get("error") {
            return Err(format!("inspector error: {error}"));
        }
        let labels = reply
            .get("labels")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "reply has no `labels`".to_string())?
            .iter()
            .map(|label| {
                label
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "a label is not a string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(InspectionVerdict {
            labels,
            backend: reply
                .get("backend")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            latency_ms: reply
                .get("latency_ms")
                .and_then(serde_json::Value::as_f64)
                .map_or(0, |ms| ms as u64),
        })
    }
}
