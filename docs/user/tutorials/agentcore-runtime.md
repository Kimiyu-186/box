# Run Box on AgentCore Runtime

The [Runtime example](../../../examples/agentcore-runtime/) runs a fixed workload or Claude Code inside a box on Amazon Bedrock AgentCore Runtime.
It deploys the same application as managed Python code and as a container image that you supply.

> **Status.** This example passed deployment and both workloads on Runtime V2 in both deployment types.
> Forced termination of the box's trusted process left descendants running in the tested baseline.
> Qualification of a combined Linux release remains separate.

## Prepare the application

Use an AWS development account with access to Runtime V2 in `us-west-2`.
Install the AgentCore CLI, AWS CLI, Node.js, Docker, Python 3.12 or later, and `uv`.
Configure an AWS profile with deployment permissions.
Use AgentCore CLI `0.13.1` and an ARM64 Linux Box bundle for this baseline:

```sh
npm install -g @aws/agentcore@0.13.1
```

Obtain a Box distribution that contains the `box` executable and its packaged helpers.
Use the archive digest and full source revision from that distribution's build receipt.
The preparation command checks the archive digest, executable architecture, and Claude Code package integrity.
It records the source revision as the operator's supplied build label.

Run the following commands from `examples/agentcore-runtime/`.
Replace the account, archive path, digest, and revision with your values:

```sh
python3 prepare.py \
  --account 123456789012 \
  --box-package /absolute/path/to/box.tar.gz \
  --box-sha256 YOUR_ARCHIVE_SHA256 \
  --box-revision YOUR_FULL_SOURCE_REVISION
```

Example output:

```text
Prepared both Runtime modes for account 123456789012 in us-west-2.
```

The command downloads the pinned Linux ARM64 Claude Code package.
Use `--claude-package /absolute/path/to/claude.tar.gz` to supply an existing copy with the same integrity value.
Generated archives, the build manifest, and the AWS target file stay outside Git.

The example uses Claude Code `2.1.305` and Claude Sonnet 4.6 through Amazon Bedrock.
Its execution role permits model invocation and short-term Bedrock bearer authentication.
The box's trusted process supplies model authentication through its egress gateway.

## Package both deployment types

Select your AWS profile and region:

```sh
export AWS_PROFILE=your-development-profile
export AWS_REGION=us-west-2
export AWS_DEFAULT_REGION=us-west-2
```

Check that the profile selects the account passed to `prepare.py`:

```sh
aws sts get-caller-identity --query Account --output text
```

Example output:

```text
123456789012
```

Validate the project and package both runtimes:

```sh
agentcore validate
agentcore package --runtime boxdemo_managed
agentcore package --runtime boxdemo_container
```

The managed package contains Python code, Python dependencies, and both executable archives.
The container package uses the pinned Amazon Linux image and installs Python 3.12.
It runs under operator identity `991:991`.

Build Python dependencies for the selected runtime.
Exclude build-machine `__pycache__` directories and `.pyc` files from a manually assembled managed ZIP.
Runtime rejected incompatible Python cache files during qualification.

## Deploy the application

Preview the infrastructure:

```sh
agentcore deploy --dry-run --target default --json
```

Inspect the generated CloudFormation template.
Both `AWS::BedrockAgentCore::Runtime` resources must specify `PlatformVersion: V2`.
The template retains resources and enables stack termination protection.

Deploy after the preview succeeds:

```sh
agentcore deploy --target default --yes --json
```

The application creates managed and container runtimes in `us-west-2`.
Record each runtime's Amazon Resource Name (ARN) from the deployment result.
Each ARN identifies the target used by the verification command.

The application uses public network mode, HTTP invocation, a 600-second idle limit, and a 3,600-second maximum lifetime.
It uses temporary runtime storage.
Allow both the runtime and its `DEFAULT` endpoint to reach `READY` before invocation.

## Verify the fixed workload

Run the verifier once for each runtime ARN:

```sh
uv run --with boto3==1.42.61 python invoke.py \
  --profile your-development-profile \
  --arn YOUR_MANAGED_RUNTIME_ARN
```

```sh
uv run --with boto3==1.42.61 python invoke.py \
  --profile your-development-profile \
  --arn YOUR_CONTAINER_RUNTIME_ARN
```

Each successful result contains `"passed": true` and a directory under `results/`.
The verifier checks Runtime V2, deployment type, endpoint version, account identity, and absence of persistent filesystem storage.
It saves the deployment details, invocation identifier, workload result, and audit records.

The workload repairs `add(a, b)` in `app.py`.
It attempts direct access to a private file and a direct overwrite of a read-only reference.
It also attempts a private-file read, file deletion, and external connection through Strands Shell.
The verification requires the expected refusals, the exact repair, and unchanged protected files.

The supplied policy permits project reads and writes.
It forbids deletion and leaves the private path and `example.com` outside its permits.
Keep these refusal checks when you change the workload or its filesystem grants.

## Run Claude Code

Use the same runtime ARN with `--agent claude`:

```sh
uv run --with boto3==1.42.61 python invoke.py \
  --profile your-development-profile \
  --arn YOUR_MANAGED_RUNTIME_ARN \
  --agent claude
```

Repeat the command with the container runtime ARN.
A successful run passes ten checks and records Claude's terminal output.
Claude repairs the function and attempts all three policy-refusal commands through its Bash tool.

The example disables Claude's own permission prompts for its generated demonstration project.
Box applies the configured policy to Bash operations.
Claude has direct read access to the project and writable access to its own state and temporary files.
Those direct filesystem grants bypass per-operation policy checks.
Read the startup disclosure when you change a grant.

The model destination is `bedrock-runtime.us-west-2.amazonaws.com`.
The policy permits model requests and refuses the separate `example.com` probe.
Use the audit record to distinguish a policy refusal from an unrelated network error.

## Read the audit and retain access

Open `result.json`, `audit.json`, and `target.json` in the printed result directory.
Claude runs also produce `terminal.log`.
The audit contains Box's policy decision records.
Direct filesystem access covered by sandbox grants does not produce those decision records.

Each verification command creates a new Runtime session identifier.
The adapter also accepts `start`, `poll`, `input`, `resize`, and `audit` operations for an interactive terminal client.
That client must preserve the Runtime session identifier and input sequence.
Retry `RetryableConflictException` during concurrent session startup.
These terminal operations belong to the example adapter.

The example retains deployed resources after verification.
Runtime session expiry removes temporary execution state.
A retained deployment can start a new session after the previous session expires.
Keep forced termination as a separate lifecycle check when qualifying a new Box bundle.

For the general Box configuration, read [getting started](../getting-started.md).
For decision record fields, read [telemetry](../telemetry.md).
For deployment resource properties, read the [AWS Runtime reference](https://docs.aws.amazon.com/AWSCloudFormation/latest/TemplateReference/aws-resource-bedrockagentcore-runtime.html).
