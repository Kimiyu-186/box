"""Print the policy decision records whose command names a program: records.py <records.jsonl> <program>."""
import json, sys

path, program = sys.argv[1], sys.argv[2]
print(f"--- policy decision records naming {program} (action | verdict | rule | args):")
try:
    lines = open(path).readlines()
except OSError as error:
    print(f"  (could not read records: {error})")
    sys.exit(0)
for line in lines:
    for batch in json.loads(line).get("resourceLogs", []):
        for scope in batch["scopeLogs"]:
            for record in scope["logRecords"]:
                a = {x["key"]: x["value"] for x in record["attributes"]}
                get = lambda k: a.get(k, {}).get("stringValue", "")
                args = [v.get("stringValue") for v in a.get("process.command_args", {}).get("arrayValue", {}).get("values", [])]
                if program not in " ".join(args or [""]) and program not in get("strands.box.policy.action"):
                    continue
                print("  ", get("strands.box.policy.action"), "|", get("strands.box.policy.verdict"), "|",
                      get("strands.box.policy.rule"), "|", " ".join(a for a in args if a))
