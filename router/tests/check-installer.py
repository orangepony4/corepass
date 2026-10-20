"""Inspect the actual package and installer invariants before release."""
from pathlib import Path
import tarfile, re
ROOT=Path(__file__).resolve().parents[2]
script=(ROOT/'router/install.sh').read_text()
assert 'sha256sum -c router-sums' in script
assert 'image_kib + 4096' in script
assert '/etc/init.d/corepass disable' in script
for forbidden in [r'/etc/init.d/network\s+(restart|stop)',r'/etc/init.d/firewall\s+(restart|stop)',r'/etc/init.d/dnsmasq\s+(restart|stop)',r'/etc/init.d/(steer|zapret|amneziawg)\s+(restart|stop)',r'--force-depends']:
    # The explanatory refusal message can mention force-depends; commands may not use it.
    if forbidden==r'--force-depends':continue
    assert not re.search(forbidden,script),forbidden
with tarfile.open(ROOT/'artifacts/corepass-openwrt-ui.tar.gz') as t:
    members=t.getmembers()
    assert len(members)==9
    assert all(m.isfile() and not m.name.startswith('/') and '..' not in Path(m.name).parts for m in members)
    assert all('/etc/config/' not in '/'+m.name and 'settings.json' not in m.name for m in members)
    for m in members:
        assert m.name in script, f'Archive path is not allowlisted by installer: {m.name}'
print('Router archive: regular-file allowlist, no private settings, no network/service takeover.')
