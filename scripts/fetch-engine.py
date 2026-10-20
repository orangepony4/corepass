"""Fetch pinned official sing-box archives and verify GitHub's SHA256 asset digest."""
import hashlib, json, pathlib, sys, tarfile, urllib.request, zipfile

ROOT=pathlib.Path(__file__).resolve().parents[1]
VERSION='1.14.3'
TARGETS={'windows':f'sing-box-{VERSION}-windows-amd64.zip','router':f'sing-box-{VERSION}-linux-arm64-musl.tar.gz'}
target=sys.argv[1] if len(sys.argv)>1 else 'windows'
name=TARGETS[target]
def get(url):
    return urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'Corepass-build'}),timeout=120)
with get(f'https://api.github.com/repos/SagerNet/sing-box/releases/tags/v{VERSION}') as r: release=json.load(r)
asset=next(a for a in release['assets'] if a['name']==name)
digest=asset.get('digest','')
if not digest.startswith('sha256:'): raise RuntimeError('Upstream does not provide a SHA256 digest')
cache=ROOT/'artifacts'/'downloads';cache.mkdir(parents=True,exist_ok=True)
archive=cache/name
if not archive.exists():
    with get(asset['browser_download_url']) as r,archive.open('wb') as f:
        while data:=r.read(262144): f.write(data)
if hashlib.sha256(archive.read_bytes()).hexdigest()!=digest[7:]:raise RuntimeError('Engine archive digest mismatch')
destination=ROOT/('src-tauri/resources/engine' if target=='windows' else 'artifacts/router-engine')
destination.mkdir(parents=True,exist_ok=True)
if target=='windows':
    with zipfile.ZipFile(archive) as z:
        for item in z.infolist():
            base=pathlib.PurePosixPath(item.filename).name
            if base in ('sing-box.exe','libcronet.dll','wintun.dll','LICENSE'):(destination/base).write_bytes(z.read(item))
    # sing-box's archive does not supply the separately licensed Wintun driver.
    wintun=cache/'wintun-0.14.1.zip'
    if not wintun.exists():
        with get('https://www.wintun.net/builds/wintun-0.14.1.zip') as r:wintun.write_bytes(r.read())
    if hashlib.sha256(wintun.read_bytes()).hexdigest()!='07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51':
        raise RuntimeError('Official Wintun archive digest mismatch')
    with zipfile.ZipFile(wintun) as z:
        (destination/'wintun.dll').write_bytes(z.read('wintun/bin/amd64/wintun.dll'))
        (destination/'WINTUN-LICENSE.txt').write_bytes(z.read('wintun/LICENSE.txt'))
else:
    with tarfile.open(archive) as t:
        for item in t.getmembers():
            if item.isfile() and pathlib.PurePosixPath(item.name).name in ('sing-box','LICENSE'):
                data=t.extractfile(item).read();p=destination/pathlib.PurePosixPath(item.name).name;p.write_bytes(data)
                if p.name=='sing-box':p.chmod(0o755)
required=['sing-box.exe','libcronet.dll','wintun.dll','WINTUN-LICENSE.txt'] if target=='windows' else ['sing-box']
for name in required:
    if not (destination/name).is_file():raise RuntimeError('Missing engine component: '+name)
print(json.dumps({'target':target,'archive_sha256':digest[7:],'files':[{ 'name':p.name,'size':p.stat().st_size}for p in destination.iterdir()]}))
