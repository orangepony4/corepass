"""Run the real installer in disposable chroots with BusyBox and local fake downloads.

This tests installer transactions, not router hardware or the real package manager.
The real ARM64 executables and SquashFS are checked by the separate native CI job.
"""
import hashlib, io, os, pathlib, re, shutil, subprocess, tarfile, tempfile

ROOT=pathlib.Path(__file__).resolve().parents[2]
BUSYBOX=pathlib.Path('/usr/bin/busybox')
assert os.geteuid()==0,'Run in CI with sudo python3'
assert BUSYBOX.is_file(),'Install busybox-static first'
CHROOT=shutil.which('chroot')
assert CHROOT,'chroot is required'

def write(root,name,text,mode=0o644):
    path=root/name.lstrip('/');path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text);path.chmod(mode)

def sandbox(case):
    with tempfile.TemporaryDirectory(prefix='corepass-install-test-') as temp:
        root=pathlib.Path(temp)
        (root/'bin').mkdir();shutil.copy2(BUSYBOX,root/'bin/busybox')
        for applet in ['id','mkdir','chmod','cat','cp','mv','rm','du','awk','grep','tar','sha256sum','mktemp','umount','gzip','gunzip']:
            (root/'bin'/applet).symlink_to('busybox')
        # BusyBox's standalone ash bypasses PATH for built-in applets (including df).
        # Use a POSIX dash shell here so dependency mocks are actually consulted.
        shutil.copy2('/bin/dash',root/'bin/sh')
        libraries=subprocess.check_output(['ldd','/bin/dash'],text=True)
        for library in re.findall(r'/[^\s()]+',libraries):
            source=pathlib.Path(library)
            if source.is_file():
                destination=root/library.lstrip('/');destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,destination)
        for d in ['tmp','overlay','dev','proc','source','etc/config','etc/steer']:(root/d).mkdir(parents=True,exist_ok=True)
        write(root,'dev/null','')
        write(root,'proc/filesystems','nodev\tproc\n\tsquashfs\n')
        write(root,'proc/mounts','')
        write(root,'etc/openwrt_release',"DISTRIB_RELEASE='24.10.4'\nDISTRIB_TARGET='mediatek/filogic'\n")
        write(root,'etc/config/network','ORIGINAL NETWORK\n')
        write(root,'etc/steer/config','ORIGINAL STEER\n')
        write(root,'bin/uname','#!/bin/sh\necho '+('mips' if case=='architecture' else 'aarch64')+'\n',0o755)
        free=32 if case=='space' else 40000
        write(root,'bin/df',f'#!/bin/sh\necho "Filesystem 1024-blocks Used Available Capacity Mounted"\necho "overlay 50000 10000 {free} 20% /overlay"\n',0o755)
        write(root,'bin/opkg',f'#!/bin/sh\necho "opkg $*" >> /calls\n'+('test "$1" != install\n' if case=='dependency' else 'exit 0\n'),0o755)
        write(root,'bin/uclient-fetch','#!/bin/sh\nwhile [ "$#" -gt 0 ]; do\n case "$1" in -O) shift; out="$1";; https://*) name="${1##*/}";; esac\n shift\ndone\ncp "/source/$name" "$out"\n',0o755)
        write(root,'etc/rc.common','#!/bin/sh\necho "$1 $2" >> /calls\ntest "$1" = /etc/init.d/corepass && test "$2" = disable\n',0o755)
        write(root,'etc/init.d/rpcd','#!/bin/sh\necho "rpcd $1" >> /calls\nexit 0\n',0o755)
        shutil.copy2(ROOT/'router/install.sh',root/'installer.sh')
        image=root/'source/corepass-openwrt-arm64.squashfs';image.write_bytes(b'fake compressed image fixture')
        payload=root/'source/corepass-openwrt-ui.tar.gz'
        with tarfile.open(ROOT/'artifacts/corepass-openwrt-ui.tar.gz') as original,tarfile.open(payload,'w:gz') as target:
            for member in original.getmembers():
                data=original.extractfile(member).read()
                if member.name=='usr/libexec/corepass/mount-engine':
                    data=(b'#!/bin/sh\nexit 17\n' if case=='mount' else b'#!/bin/sh\nmkdir -p /usr/lib/corepass/runtime\nprintf "#!/bin/sh\\nexit 0\\n" > /usr/lib/corepass/runtime/corepass-router\nchmod 755 /usr/lib/corepass/runtime/corepass-router\n')
                member.size=len(data);target.addfile(member,io.BytesIO(data))
        sums=''
        for path in [image,payload]:sums+=hashlib.sha256(path.read_bytes()).hexdigest()+'  '+path.name+'\n'
        # The release contains a Windows asset too; the router must only verify its own downloads.
        sums+='a'*64+'  Corepass-0.3.0-windows-x64.zip\n'
        if case=='checksum':sums=sums.replace(hashlib.sha256(image.read_bytes()).hexdigest(),'0'*64)
        write(root,'source/SHA256SUMS',sums)
        result=subprocess.run([CHROOT,str(root),'/bin/sh','/installer.sh'],capture_output=True,text=True,timeout=15,env={'PATH':'/bin','LC_ALL':'C'})
        success=case=='success'
        assert (result.returncode==0)==success,(case,result.returncode,result.stdout,result.stderr)
        assert (root/'etc/config/network').read_text()=='ORIGINAL NETWORK\n'
        assert (root/'etc/steer/config').read_text()=='ORIGINAL STEER\n'
        if success:
            import json
            config=json.loads((root/'etc/corepass/settings.json').read_text());assert config['enabled'] is False
            calls=(root/'calls').read_text();assert '/etc/init.d/corepass disable' in calls and 'rpcd restart' in calls
            assert '/etc/init.d/corepass start' not in calls
            assert (root/'usr/lib/corepass/version').read_text().strip()=='0.3.0'
        else:
            assert not (root/'usr/lib/corepass/version').exists()
            assert not (root/'usr/lib/corepass/engine.squashfs').exists()
        print(case+': passed')

for case in ['success','architecture','checksum','space','dependency','mount']:sandbox(case)
