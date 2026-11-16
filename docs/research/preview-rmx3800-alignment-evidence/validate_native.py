"""Execute original preview planning/state-transition instructions, without I/O.
Contexts are synthetic protocol-test inputs, not device authorization evidence.
No instruction hooks, patched code, cryptography adapters, or service calls.
"""
import hashlib, json, struct
from pathlib import Path
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64
from unicorn.x86_const import UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_RSP, UC_X86_REG_RIP, UC_X86_REG_RAX
ROOT = Path(__file__).resolve().parents[2]
SAMPLE = ROOT/'runs/anteasy-oplustool-preview/extracted/Oplusdll/Oplus_Qualcomm.dll'
pe = pefile.PE(str(SAMPLE)); image = pe.get_memory_mapped_image(); base = pe.OPTIONAL_HEADER.ImageBase
DATA, STACK, STOP = 0x30000000, 0x20000000, 0x40000000

def execute(address, blob, args):
    uc = Uc(UC_ARCH_X86, UC_MODE_64)
    uc.mem_map(base, (len(image)+4095)&~4095); uc.mem_write(base,image)
    uc.mem_map(DATA, 0x10000); uc.mem_write(DATA,blob)
    uc.mem_map(STACK, 0x10000); uc.mem_map(STOP,4096)
    sp = STACK+0xff08; uc.mem_write(sp,struct.pack('<Q',STOP));uc.reg_write(UC_X86_REG_RSP,sp)
    for reg,value in zip([UC_X86_REG_RCX,UC_X86_REG_RDX,UC_X86_REG_R8],args):uc.reg_write(reg,value)
    uc.emu_start(address,STOP,count=100000)
    assert uc.reg_read(UC_X86_REG_RIP)==STOP
    return uc.reg_read(UC_X86_REG_RAX), bytes(uc.mem_read(DATA,0x2000))

plans=[]
for size,sectors,mode,force in [(8192,16,1,0),(4096,16,1,0),(1155885,8356,1,0),(1110356,1536,1,0),(1,16,1,0),(8192,1,1,0),(8192,16,1,1),(8192,16,0,0),(8192,1,0,0)]:
    args=struct.pack('<IIIIQQQ',40,mode,force,4096,size,sectors if mode else 0,sectors if not mode else 0)
    code,mem=execute(0x180053da0,args,[DATA,DATA+0x1000])
    expected=(size+4095)//4096
    expected=sectors if mode and force else min(expected,sectors) if mode else expected
    expected_code=0x25 if not mode and expected>sectors else 0
    assert code==expected_code
    values=struct.unpack('<IIQQQQQ',mem[0x1000:0x1030])
    if code==0:assert values[2:]==(expected,expected*4096,min(size,expected*4096),max(0,expected*4096-size),max(0,size-expected*4096))
    plans.append(dict(size=size,sectors=sectors,mode=mode,force=force,result=code,output=values))
transitions=[]
for state,amount,expected in [(1,512,2),(2,0,3),(3,4096,5),(6,0,7)]:
    context=bytearray(0xd0)
    struct.pack_into('<IIQQQQII',context,0,1,0,512,4096,0,0,state,0)
    code,mem=execute(0x1800424c0,bytes(context),[DATA,state,amount])
    after=struct.unpack_from('<I',mem,0x28)[0]
    assert code==0 and after==expected
    transitions.append(dict(state=state,bytes=amount,next=after,result=code))
result=dict(sample_sha256=hashlib.sha256(SAMPLE.read_bytes()).hexdigest(),scope=__doc__,raw_plans=plans,digest_transitions=transitions)
Path(__file__).with_name('native-validation.json').write_text(json.dumps(result,indent=2)+'\n')
print('PASS: 9 raw plans and 4 DIGEST transitions; original machine code, no hooks')
