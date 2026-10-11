# aws pack corpus

Expected parses for `aws.pack.toml` under the shell grammar in `crates/policy/src/pack/grammar.rs`.
`run.py` checks the action and the effects of every row.
A row shows only fields that differ from their defaults. `fx` is `context.input.effects`.
Rows marked "verified" were checked against aws-cli 2.36 on this host, with no credentials and
`--endpoint-url http://127.0.0.1:9`, so that each line got past argument parsing and stopped
there.

| # | Command line | Action | Fields | fx |
|---|---|---|---|---|
| 1 | `aws sts get-caller-identity` | sts_get_caller_identity | — | read_only |
| 2 | `aws --profile prod sts get-caller-identity` | sts_get_caller_identity | profile=`prod` | read_only |
| 3 | `aws sts get-caller-identity --profile=prod --region us-east-1 --output json` | sts_get_caller_identity | profile, region, output (globals after the operation) | read_only |
| 4 | `aws sts get-caller-identity --query Account --output text` | sts_get_caller_identity | query=`Account`, output=`text` | read_only |
| 5 | `aws s3 --region us-west-2 ls` | s3_ls | region; a global between the two command words (verified) | read_only |
| 6 | `aws s3 rm s3://logs/2026/app.log` | s3_rm | path=`s3://logs/2026/app.log` | delete_data |
| 7 | `aws s3 rm s3://logs/ --recursive --dryrun` | s3_rm | recursive, dryrun | — |
| 8 | `aws s3 rm s3://logs/ --recursive --dry` | unmodeled | aws reads `--dry` as `--dryrun` (verified); no prefix matching | — |
| 9 | `aws s3 rm --profile=prod s3://logs/k` | s3_rm | profile=`prod`, path | delete_data |
| 10 | `aws s3 rm --exclude '*' --include '*.log' --recursive s3://logs/` | s3_rm | exclude={`*`}, include={`*.log`}, recursive, path | delete_data |
| 11 | `aws s3 rm` | unmodeled | required operand `<path>` missing | — |
| 12 | `aws s3 rm help` | s3_rm | path=`help`. aws prints help instead; over-restriction, harmless | delete_data |
| 13 | `aws s3 cp ./site s3://www/ --recursive --acl public-read` | s3_cp | source=`./site`, destination, recursive, acl | write_data, make_public |
| 14 | `aws s3 cp s3://data/dump.sql .` | s3_cp | source, destination=`.` | read_data |
| 15 | `aws s3 cp s3://a/x s3://b/x` | s3_cp | source, destination | read_data, write_data |
| 16 | `aws s3 cp - s3://b/k` | s3_cp | source=`-` (stdin; a lone `-` is an operand) | write_data |
| 17 | `aws s3 cp a s3://b/ --grants read=uri=http://acs.amazonaws.com/groups/global/AllUsers` | s3_cp | grants={…AllUsers} (EXT-6 `many`, EXT-5 `like` on an option) | write_data, make_public |
| 18 | `aws s3 cp --sse AES256 a s3://b/` | unmodeled | `--sse` left out (argparse optional value takes the next word) | — |
| 19 | `aws s3 mv s3://a/x ./x` | s3_mv | source, destination | delete_data, read_data |
| 20 | `aws s3 mv ./x s3://a/x` | s3_mv | the local source is deleted | delete_data, write_data |
| 21 | `aws s3 sync . s3://site --delete` | s3_sync | delete | write_data, delete_data |
| 22 | `aws s3 sync s3://site ./local --delete --dryrun` | s3_sync | delete, dryrun | — |
| 23 | `aws s3 rb s3://old --force` | s3_rb | path, force | delete_data |
| 24 | `aws s3api delete-bucket --bucket prod-logs` | s3api_delete_bucket | bucket=`prod-logs` | delete_data |
| 25 | `aws s3api delete-bucket --bucket=prod-logs --region eu-west-1` | s3api_delete_bucket | bucket, region | delete_data |
| 26 | `aws s3api delete-bucket --bucket file://name.txt` | s3api_delete_bucket | bucket=`file://name.txt`. aws loads the value from the file; the parser sees the name only (EXT-10 is not implemented) | delete_data |
| 27 | `aws s3api delete-bucket --cli-input-json file://in.json` | unmodeled | `--cli-input-json` not declared | — |
| 28 | `aws s3api delete-bucket --bucket a --bucket b` | unmodeled | non-repeated option given twice (argparse: last wins) | — |
| 29 | `aws s3api delete-bucket` | s3api_delete_bucket | bucket absent. aws refuses the line; required options are not checked | delete_data |
| 30 | `aws ec2 terminate-instances --instance-ids i-1 i-2 --dry-run` | ec2_terminate_instances | instance_ids={`i-1`,`i-2`}, dry_run | — |
| 31 | `aws ec2 terminate-instances --instance-ids i-1 i-2` | ec2_terminate_instances | instance_ids={`i-1`,`i-2`} | terminate_compute |
| 32 | `aws ec2 terminate-instances --instance-ids=i-1 i-2` | unmodeled | `=` gives `many` exactly one value; `i-2` is then a stray operand | — |
| 33 | `aws ec2 terminate-instances --instance-ids i-1 --dry-run --no-dry-run` | unmodeled | negation not declared (aws: last wins, so it would terminate) | — |
| 34 | `aws ec2 describe-instances --filters Name=tag:env,Values=prod --max-items 10` | ec2_describe_instances | filters={…}, max_items=`10` | read_only |
| 35 | `aws ec2 describe-instances --max-items ten` | ec2_describe_instances | max_items=`ten` (no `type = "long"`) | read_only |
| 36 | `aws sts assume-role --role-arn arn:aws:iam::1:role/admin --role-session-name s --duration-seconds 3600` | sts_assume_role | role_arn, role_session_name, duration_seconds=`3600` | assume_role |
| 37 | `aws iam list-roles --path-prefix /service/` | iam_list_roles | path_prefix | read_only |
| 38 | `aws iam create-access-key --user-name ci-bot` | iam_create_access_key | user_name | modify_iam, create_credentials |
| 39 | `aws iam attach-role-policy --role-name app --policy-arn arn:aws:iam::aws:policy/AdministratorAccess` | iam_attach_role_policy | role_name, policy_arn | modify_iam |
| 40 | `aws iam put-role-policy --role-name app --policy-name inline --policy-document file://p.json` | iam_put_role_policy | policy_document=`file://p.json` (the parser sees the file name only) | modify_iam |
| 41 | `aws cloudformation delete-stack --stack-name prod-api --retain-resources DataBucket Logs` | cloudformation_delete_stack | stack_name, retain_resources={`DataBucket`,`Logs`} | delete_infrastructure, delete_data |
| 42 | `aws lambda invoke --function-name fn --payload file://e.json out.json` | lambda_invoke | function_name, payload, outfile=`out.json` | invoke_function |
| 43 | `aws lambda invoke --function-name fn --invocation-type DryRun out.json` | lambda_invoke | invocation_type=`DryRun` | — |
| 44 | `aws lambda invoke --function-name fn` | unmodeled | required operand `<outfile>` missing | — |
| 45 | `aws --endpoint-url https://collector.example s3 ls` | s3_ls | endpoint_url | read_only, custom_endpoint |
| 46 | `aws s3 ls --no-verify-ssl` | s3_ls | no_verify_ssl | read_only, insecure_tls |
| 47 | `aws --cli-auto-prompt s3 rm s3://b/k` | unmodeled | interactive edit before run; not declared | — |
| 48 | `aws S3 ls` | unmodeled | command words are case-sensitive (aws: "invalid choice") | — |
| 49 | `aws dynamodb delete-table --table-name t` | unmodeled | subcommand=`dynamodb delete-table` | — |
| 50 | `aws configure set region us-east-1` | unmodeled | subcommand=`configure set` | — |
| 51 | `aws s3 rm s3://b/k --profile` | unmodeled | value missing | — |
