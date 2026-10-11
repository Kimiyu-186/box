"""Run every corpus row through the parse_pack example: run.py <parse_pack binary> <packs dir> <corpus dir>.

   cargo build -p strands-box-policy --examples
   python3 examples/packs/corpus/run.py target/debug/examples/parse_pack examples/packs examples/packs/corpus
"""
import re, shlex, subprocess, sys, json
BIN = sys.argv[1]; PACKS = sys.argv[2]; DRAFT = sys.argv[3]
B = {'update_lockfile','network','run_build_scripts'}; T = B | {'execute_built_code'}
def fx(text):
    text = text.strip()
    if text in ('—', '-', ''): return set()
    m = re.match(r'^([BT])\s*([+−-])\s*(.*)$', text)
    if m:
        base = set(B if m.group(1)=='B' else T)
        extra = {x.strip() for x in m.group(3).split(',') if x.strip()}
        return base | extra if m.group(2)=='+' else base - extra
    if text in ('B','T'): return set(B if text=='B' else T)
    return {x.strip() for x in text.split(',') if x.strip()}
total = ok = 0; skipped = []
for tool in ('git','cargo','aws'):
    for line in open(f'{DRAFT}/{tool}.corpus.md'):
        if not re.match(r'^\| \d+ ', line): continue
        cells = [c.strip() for c in line.strip().strip('|').split('|')]
        num = int(cells[0]); cmdcell = cells[1]; action = cells[2]; fields = cells[3]; effects = fx(cells[4])
        cmds = re.findall(r'`([^`]+)`', cmdcell)
        if 'with `.cargo' in cmdcell: cmds = cmds[:1]

        for cmd in cmds:
            if not cmd.startswith(tool) : continue
            argv = shlex.split(cmd)
            out = json.loads(subprocess.run([BIN, f'{PACKS}/{tool}.pack.toml', '--', *argv], capture_output=True, text=True).stdout)
            got_action = out['action']; got_fx = set(out.get('effects', []))
            total += 1
            good = got_action == action and (action == 'unmodeled' or got_fx == effects)
            ok += good
            mark = 'PASS' if good else 'FAIL'
            print(f"{mark} {tool}#{num} {cmd!r}: got {got_action} {sorted(got_fx)} {json.dumps(out.get('fields', {}), separators=(',',':'))}")
            if not good: print(f"      expected {action} {sorted(effects)} | {fields}")
print(f"\n{ok}/{total} pass")
