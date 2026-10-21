# Cloud deployment profiles

Cloud deployment profiles record the environment around Box for three Amazon Linux 2023 ARM64 deployment targets.
A deployment profile describes the service configuration. It does not change the Box policy or sandbox grants.

> **Status.** Qualification configurations as of October 21, 2026. Runtime qualification failed. Lambda passed the listed scenarios with root and `ALL` capabilities. Deployment scripts remain pending.

These measurements apply to Box revision `913f5367f57424b3bda65dc4a05278a5664f635d`.
The candidate archive is `box-v0.1.0-aarch64-unknown-linux-gnu.tar.gz`.
Its SHA-256 checksum is `89e8547d009731cd3f4a671e15b0f3485a254cc0085e201484bd25e1b3047271`.
This checksum identifies the tested candidate, not a published download.

Use these configurations to plan qualification in your environment.
Repeat qualification after changes to Box, the service runtime, the image, or the workload.

<a id="shared-requirements"></a>
## Shared requirements

The following requirements apply to the tested Box executables.
The application and its tools can require additional libraries and interpreters.
The recorded versions identify reproduction targets. They do not establish minimum kernel versions or resource capacity.

| Requirement | Required configuration | Reason or verification |
| --- | --- | --- |
| Architecture | Linux ARM64, reported as `aarch64`. | The binaries and system-call filters depend on the architecture. Check `uname -m`. |
| Executables | Keep `box`, `strands-box-contain-trampoline`, and `strands-box-sock-alias` from the same build. | Box uses the helpers during setup and command execution. Preserve executable permissions. |
| C runtime | Provide `glibc`, including `libc.so.6`, `libm.so.6`, and `/lib/ld-linux-aarch64.so.1`. | The candidate requires GLIBC symbols through version `2.34`. Record `rpm -q glibc`. |
| GCC runtime | Provide `libgcc_s.so.1` from `libgcc`. | The candidate links this library. Record `rpm -q libgcc`. The oldest compatible package remains unmeasured. |
| Namespaces | Permit creation of user, mount, process, interprocess communication, and hostname namespaces. Permit a private network namespace for default networking. | Box needs all these operations for its default setup. A successful user-namespace probe alone is insufficient. |
| Identity and mounts | Permit identity mapping, bind mounts, private temporary filesystems, a process filesystem, and root replacement. | Box constructs each sandbox's filesystem and process view. An outer service restriction can refuse these operations. |
| Workload restrictions | Permit capability removal, `no_new_privs`, and seccomp filter installation. | Box removes setup privileges and restricts workload system calls. Verify the resulting workload process status. |
| Private state | Provide writable `/tmp` and separate private directories for Box state and each task. | Keep Box control state outside the workload's writable filesystem grants. |
| System interfaces | Provide `/dev/urandom`, `/dev/null`, required `/proc` interfaces, and Unix-domain sockets. | Setup uses these interfaces for temporary names, process inspection, identity mapping, and local communication. |
| Application dependencies | Include the application, its interpreters, libraries, certificates, and tool dependencies. | A program that starts outside Box can still fail inside its sandbox. Test actual contained execution. |

Box uses temporary Linux capabilities while it constructs namespaces and mounts.
`CAP_SYS_ADMIN` applies inside the new user namespace. It does not imply administrative access to the surrounding OS.
Box uses `CAP_SETPCAP` to remove setup capabilities before workload execution.
The localhost relay also uses `CAP_NET_ADMIN` to enable its private loopback interface.
These operations do not constitute a general container `cap-add` recipe.

<a id="runtime-managed-code"></a>
## AgentCore Runtime managed code

[Amazon Bedrock AgentCore Runtime](https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/agents-tools-runtime.html)
supplies the Python runtime for this profile.
The deployment archive supplies the application, Box executables, policy, and workload dependencies.

| Setting | Tested configuration | Operator action |
| --- | --- | --- |
| Service runtime | Runtime platform `V2`, managed `PYTHON_3_12`, Python `3.12.12`. | Select the matching runtime when reproducing these measurements. |
| Operating system | Amazon Linux `2023.12.20260928`. | Record the service's actual `/etc/os-release`. The service controls this filesystem. |
| Kernel | `6.1.166-24.303.amzn2023.aarch64`. | Record `uname -r` inside the deployed service. |
| Operator | User and group ID `991`. Effective and permitted capability sets were empty. | Run Box under the service identity. Verify privileges from the actual service process. |
| Application | HTTP port `8080`, `GET /ping`, and `POST /invocations`. | Start the application server through the runtime entry point. |
| Files and state | Bundled payload extracted into a private `/tmp` directory. Persistent session filesystem disabled. | Verify payload hashes. Create separate task workspaces and Box state directories. |

The service runtime determines the available system libraries.
Record their versions during qualification. The current receipts do not provide a complete Runtime package inventory.

<a id="runtime-container"></a>
## AgentCore Runtime customer container

This profile supplies the application and its dependencies in an ARM64 container image.
AgentCore Runtime supplies the kernel and outer execution restrictions.

| Setting | Tested configuration | Operator action |
| --- | --- | --- |
| Service runtime | Runtime platform `V2`. | Qualify the deployed image inside the actual service. |
| Image filesystem | Amazon Linux `2023.12.20261016`, Python `3.9.25`. | Install Box, the application, and their runtime dependencies in the image. |
| Kernel | `6.1.166-24.303.amzn2023.aarch64`. | Record the service kernel separately from the image filesystem. |
| Operator | Container `USER 991:991`. Effective and permitted capability sets were empty. | Create the account during the image build. Start the application as that account. |
| Application | HTTP port `8080`, `GET /ping`, and `POST /invocations`. | Configure the image entry point to start the application server. |
| Files and state | Application files under `/app`, private task data under `/tmp`. Persistent session filesystem disabled. | Give the operator access to its private state and workspaces. |

The tested image starts from
`public.ecr.aws/amazonlinux/amazonlinux@sha256:c07ab0773eddf9449c0994a87d8148300db98bdf8d8dad672a9dc26321c104fc`.
The test image also installs `python3`, `shadow-utils`, `procps-ng`, and `git`.
These packages support the test application and its probes. They are not universal Box dependencies.

<a id="lambda-microvm"></a>
## Lambda MicroVM

This profile supplies an application image and lifecycle hooks for a
[Lambda MicroVM](https://docs.aws.amazon.com/lambda/latest/dg/microvms-images.html).
The recorded configuration uses root and broad service capabilities.
Minimum privileges remain unqualified.

| Setting | Tested configuration | Operator action |
| --- | --- | --- |
| Managed base | `al2023-1`, version `2`. | Record the managed base version separately from the application image. |
| Operating system | Amazon Linux `2023.12.20261016`, ARM64. | Record the actual filesystem version and architecture. |
| Kernel | `6.1.166-24.303.amzn2023.aarch64`. | Verify inside the running MicroVM. |
| Operator and capabilities | User ID `0`, `additionalOsCapabilities: ["ALL"]`. | Treat this as the tested qualification configuration. The default capability profile failed at a process-filesystem mount. |
| Application | Application port `8080`, lifecycle hook port `9000`. | Connect application startup and lifecycle handling to their respective listeners. |
| Lifecycle hooks | Ready, validate, run, resume, suspend, and terminate hooks. | Track the active task across lifecycle transitions. Verify task cleanup at termination. |
| Files and capacity | Private task directories beneath `/work`. Real-agent tests used `4096` MiB. | Separate task state. Measure capacity for your workload. This memory value is not a minimum. |

The qualification image includes Python 3.12 and application-specific dependencies.
The tests exercise resumed execution, permitted operations, denied operations, and cleanup.
They do not establish a non-root deployment or a minimum capability set.

<a id="qualification-results"></a>
## Qualification results

Both Runtime profiles passed startup and the recorded namespace and workload restriction checks.
The contained workload reported empty capability sets, `NoNewPrivs=1`, and `Seccomp=2`.
Their broader suite recorded 101 passes, three failures, two errors, and 23 skips.
Supplemental checks recorded ten passes and four failures.

Forced termination of only the box's trusted process left four matching processes alive after four seconds.
Script and library discovery also caused program startup failures.
The original filesystem failures used paths inside the workload's writable private `/tmp`.
Follow-up controls corrected that interpretation. Those failures did not demonstrate writes escaping into the surrounding OS.

Lambda real-agent tests completed 14 checks each for Claude Code and Codex CLI.
They verified helper completion, permitted network requests, policy denials, protected file contents, and absence of observed surviving task processes.
These scenarios provide narrower evidence than the Runtime component suite.
Their results do not establish general package qualification.

Keep each profile's results attached to its exact candidate and service configuration.
For Box configuration, read the [configuration reference](./config.md).
For the examples entry, see [cloud deployment profiles](../../examples/cloud-deployments/).

## Terms

| Term | Definition | Notes |
| --- | --- | --- |
| Namespace | A Linux mechanism that gives processes a separate view of selected resources. | Examples include process IDs, mounts, and network interfaces. |
| Capability | A Linux privilege for a specific class of operations. | Its namespace scope matters. |
| Seccomp | A Linux mechanism that filters system calls. | `Seccomp=2` means filter mode. |
| `no_new_privs` | A Linux process setting that prevents execution from granting new privileges. | The process status shows `NoNewPrivs=1`. |
| Lifecycle hook | Application code that a service calls during an execution transition. | Examples include resume and terminate. |
| Qualification | Tests required to accept a candidate on a specified target. | A completed test run can fail qualification. |
