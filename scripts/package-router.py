"""Build a small archive with an explicit regular-file allowlist; no user configuration."""
from pathlib import Path
import tarfile
ROOT=Path(__file__).resolve().parents[1]
dest=ROOT/'artifacts/corepass-openwrt-ui.tar.gz'
dest.parent.mkdir(parents=True,exist_ok=True)
base=ROOT/'router/root'
with tarfile.open(dest,'w:gz') as archive:
    files=sorted(p for p in base.rglob('*') if p.is_file())
    for path in files:
        if path.is_symlink():raise RuntimeError('Symlinks are not accepted in the router package')
        name=path.relative_to(base).as_posix()
        info=archive.gettarinfo(str(path),name)
        info.uid=info.gid=0;info.uname=info.gname='root';info.mtime=0
        info.mode=0o755 if name.startswith(('etc/init.d/','usr/libexec/')) else 0o644
        with path.open('rb') as stream:archive.addfile(info,stream)
    for name in ['LICENSE','THIRD-PARTY.md']:
        path=ROOT/name;info=archive.gettarinfo(str(path),'usr/share/corepass/'+name)
        info.uid=info.gid=0;info.uname=info.gname='root';info.mtime=0;info.mode=0o644
        with path.open('rb') as stream:archive.addfile(info,stream)
print(dest)
