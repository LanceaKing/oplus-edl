import json
import pathlib
import binaryninja as bn

root = pathlib.Path('/Users/Apple/Documents/oplus-edl/runs/msmdownloadtool-analysis')
out = root / 'op6' / 'bn'
out.mkdir(exist_ok=True)
bv.update_analysis_and_wait()
metadata = {'tool': bn.core_version(), 'filename': bv.file.original_filename,
            'entry': hex(bv.entry_point), 'architecture': bv.arch.name,
            'functions': len(bv.functions),
            'sections': [{'name': s.name, 'start': hex(s.start), 'length': s.length} for s in bv.sections.values()]}
(out/'metadata.json').write_text(json.dumps(metadata, indent=2))
(out/'strings.json').write_text(json.dumps([{'address': hex(s.start), 'text': s.value} for s in bv.strings], ensure_ascii=False, indent=2))
(out/'symbols.json').write_text(json.dumps([{'address': hex(s.address), 'name': s.full_name, 'type': str(s.type)} for ss in bv.symbols.values() for s in ss], indent=2))
(out/'functions.json').write_text(json.dumps([{'address': hex(f.start), 'name': f.name, 'size': f.total_bytes, 'callers': [hex(x.start) for x in f.callers], 'callees': [hex(x.start) for x in f.callees]} for f in bv.functions], indent=2))
xrefs = [{'address':hex(s.start), 'text':s.value, 'refs':[{'address':hex(x.address), 'function':hex(x.function.start)} for x in bv.get_code_refs(s.start)]} for s in bv.strings]
(out/'string-xrefs.json').write_text(json.dumps(xrefs,ensure_ascii=False,indent=2))
(out/'hlil').mkdir(exist_ok=True)
for f in bv.functions:
    if f:
        (out/'hlil'/(hex(f.start)+'.txt')).write_text('\n'.join(f'{l.address:08x} {l}' for l in f.hlil.root.lines))
bv.create_database(str(out/'analysis.bndb'))
print('EXPORTED', str(out), metadata)
