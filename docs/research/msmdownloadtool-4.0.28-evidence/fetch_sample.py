import hashlib
import io
import json
import pathlib
import urllib.request
import zipfile
import subprocess
import sys
import re

ROOT = pathlib.Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else pathlib.Path(__file__).resolve().parent
ROOT.mkdir(parents=True, exist_ok=True)
URL = 'https://onepluscommunityserver.com/list/Unbrick_Tools/OnePlus_8/Global_IN21AA/R/OnePlus_8_Global_OxygenOS_11.0.4.4.zip'

if len(sys.argv) > 1: URL = sys.argv[1]

class Remote(io.RawIOBase):
    def __init__(self, url):
        self.url, self.pos, self.total_read = url, 0, 0
        raw = subprocess.check_output(['curl', '--fail', '-sSIL', '--max-time', '45', url]).decode()
        (ROOT / 'archive.headers').write_text(raw)
        self.size = int(re.findall(r'content-length: (\d+)', raw, re.I)[-1])
        self.headers = {'metadata_source': 'archive.headers', 'transport': 'curl with TLS verification'}
    def seekable(self): return True
    def seek(self, n, whence=0):
        self.pos = n if whence == 0 else self.pos + n if whence == 1 else self.size + n
        return self.pos
    def tell(self): return self.pos
    def read(self, n=-1):
        n = self.size - self.pos if n < 0 else min(n, self.size - self.pos)
        assert 0 <= n <= 64 * 1024 * 1024, n
        chunks = []
        while n:
            count = min(n, 4 * 1024 * 1024)
            start = self.pos
            header_path = ROOT / 'last-range.headers'
            b = subprocess.check_output(['curl', '--fail', '-sS', '--retry', '2', '--max-time', '90', '--range', f'{start}-{start+count-1}', '-D', str(header_path), self.url])
            headers = header_path.read_text()
            assert f'content-range: bytes {start}-{start+count-1}/{self.size}' in headers.lower(), headers
            assert len(b) == count, (len(b), count)
            self.pos += count
            self.total_read += count
            n -= count
            chunks.append(b)
        return b''.join(chunks)

r = Remote(URL)
z = zipfile.ZipFile(r)
out = {'url': URL, 'archive_size': r.size, 'headers': r.headers,
       'entries': [{'name': i.filename, 'size': i.file_size, 'compressed': i.compress_size, 'crc32': f'{i.CRC:08x}', 'header_offset': i.header_offset} for i in z.infolist()]}
(ROOT / 'archive-index.json').write_text(json.dumps(out, indent=2))
print(json.dumps(out['entries'], indent=2), flush=True)
out['selected_members'] = []
for i in z.infolist():
    if i.is_dir() or i.file_size > 64*1024*1024:
        continue
    if len(sys.argv) > 3 and not i.filename.lower().endswith(('.exe','.dll','.ini','.xml','.txt','.md5')): continue
    data = z.read(i)
    dest = ROOT / 'sample' / pathlib.PurePosixPath(i.filename).name
    dest.parent.mkdir(parents=True, exist_ok=True)
    assert not dest.exists(), dest
    dest.write_bytes(data)
    item = {'archive_member': i.filename, 'local_path': str(dest), 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'md5': hashlib.md5(data).hexdigest(), 'crc32_verified': True}
    out['selected_members'].append(item)
    print(json.dumps(item), flush=True)
out['range_bytes_read'] = r.total_read
(ROOT / 'acquisition.json').write_text(json.dumps(out, indent=2))
