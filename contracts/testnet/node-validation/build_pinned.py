#!/usr/bin/env python3
"""Inspect/build the exact native nearcore with disk and memory admission checks.

No downloads, compiler/profile substitutions or host library installation.
The resource margins below are conservative local build policy, not upstream
minimum requirements or a prediction of the final binary size.
"""
import argparse
import os
import platform
import shutil
import signal
import subprocess
import time
import tomllib
from common import *
from source_tree import verify_tree

GIB=1024**3

def memory_available():
    values=dict(line.split(':',1) for line in Path('/proc/meminfo').read_text().splitlines())
    return int(values['MemAvailable'].split()[0])*1024

def inspect(source,archive,component):
    overlay={}
    provenance=source/'nft-validation-source.json'
    if provenance.exists():
        overlay=load(provenance)['overlay_files']
        # Provenance cannot bless arbitrary runtime/source changes.
        from prepare_testloop import prepare
        import tempfile
        with tempfile.TemporaryDirectory(prefix='nft-overlay-check-') as tmp:
            prepare(archive,Path(tmp)/'source')
            expected=load(Path(tmp)/'source'/('nearcore-'+COMMIT)/'nft-validation-source.json')['overlay_files']
        require(overlay==expected,'overlay does not match reviewed local adapter')
    tree=verify_tree(archive,source,overlay)
    toolchain=tomllib.loads((source/'rust-toolchain.toml').read_text())
    require(toolchain['toolchain']['channel']=='1.95.0','source Rust pin mismatch')
    workspace=tomllib.loads((source/'Cargo.toml').read_text())
    require(workspace['workspace']['package']['version']==VERSION,'workspace version mismatch')
    compiler=subprocess.run(['rustc','+1.95.0','-Vv'],capture_output=True,text=True,check=True).stdout
    require('\nrelease: 1.95.0\n' in compiler and '\nhost: x86_64-unknown-linux-gnu\n' in compiler,'native Rust 1.95.0 required')
    reasons=[]; git_commit=None
    if component=='neard':
        result=subprocess.run(['git','-C',str(source),'rev-parse','HEAD'],capture_output=True,text=True)
        git_commit=result.stdout.strip()
        if result.returncode or git_commit!=COMMIT:
            reasons.append('neard requires genuine pinned Git HEAD to embed its commit; archive alone reports unknown')
    elif not overlay:
        reasons.append('prepare_testloop.py overlay is required for this component')
    dependencies={name:shutil.which(name) for name in ('cc','c++','cmake','make','pkg-config','clang')}
    reasons += ['missing native build dependency: '+name for name,path in dependencies.items() if not path]
    openssl=subprocess.run(['pkg-config','--modversion','openssl'],capture_output=True,text=True)
    if openssl.returncode: reasons.append('OpenSSL development metadata missing')
    # Fat LTO can consume considerable memory at final link. Leave workstation
    # headroom instead of relying on OOM or deleting user-owned build caches.
    free=shutil.disk_usage(source).free; memory=memory_available()
    required_disk=30*GIB; required_memory=6*GIB
    if free<required_disk: reasons.append('less than 30 GiB free: 24 GiB build allowance plus 6 GiB reserve')
    if memory<required_memory: reasons.append('less than 6 GiB MemAvailable for the pinned build/link profile')
    command=['cargo','+1.95.0']
    if component=='neard': command+=['build','--locked','--release','-p','neard','--features','sandbox']
    elif component=='exporter': command+=['build','--locked','--release','-p','near-store','--bin','nft-receipt-exporter']
    else: command+=['test','--locked','--release','-p','test-loop-tests','--features','test_features','--lib','--no-run','--message-format=json']
    return {'source':tree,'git_commit':git_commit,'rustc':compiler,'cargo_lock_sha256':digest(source/'Cargo.lock'),
        'component':component,'command':command,'release_profile':workspace['profile']['release'],
        'dependencies':dependencies,'openssl':openssl.stdout.strip(),'host':platform.platform(),
        'disk_free_bytes':free,'memory_available_bytes':memory,
        'resource_policy':{'required_free_disk_bytes':required_disk,'required_available_memory_bytes':required_memory,
                           'stop_free_disk_bytes':6*GIB,'jobs':1,'basis':'conservative local policy; build size unmeasured'},
        'blockers':reasons,'build_status':'NOT_RUN','executable_sha256':None}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,required=True)
    p.add_argument('--archive',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True,help='fresh build/preflight report directory')
    p.add_argument('--component',choices=['neard','testloop','exporter'],default='neard')
    p.add_argument('--build',action='store_true')
    a=p.parse_args(); require(not a.output.exists(),'output must be fresh')
    a.output.mkdir(parents=True); a.source=a.source.resolve()
    try: report=inspect(a.source,a.archive,a.component)
    except (ValueError,OSError,KeyError,subprocess.SubprocessError) as exc:
        report={'build_status':'NOT_RUN','executable_sha256':None,'blockers':[str(exc)]}
    save(a.output/'build.json',report)
    if report['blockers'] or not a.build:
        print(json.dumps(report,indent=2)); return 2 if report['blockers'] else 0
    env=os.environ.copy()
    for key in ('RUSTC','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','CARGO_TARGET_DIR',
                'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS',*[name for name in env if name.startswith('CARGO_PROFILE_')]):
        require(not env.get(key),'remove compiler/profile override: '+key)
    env.update(CARGO_BUILD_JOBS='1',CARGO_BUILD_TARGET='x86_64-unknown-linux-gnu',CARGO_INCREMENTAL='0')
    started=time.monotonic()
    with (a.output/'build.log').open('x') as log:
        child=subprocess.Popen(report['command'],cwd=a.source,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try:
            while child.poll() is None:
                if shutil.disk_usage(a.source).free<6*GIB:
                    report['blockers'].append('build stopped at 6 GiB free-disk reserve')
                    os.killpg(child.pid,signal.SIGTERM)
                    break
                time.sleep(1)
            try: child.wait(timeout=20)
            except subprocess.TimeoutExpired: os.killpg(child.pid,signal.SIGKILL); child.wait()
        finally:
            if child.poll() is None: os.killpg(child.pid,signal.SIGTERM); child.wait()
    report.update(elapsed_seconds=round(time.monotonic()-started,3),exit_code=child.returncode,
                  build_status='PASS' if child.returncode==0 else 'FAIL')
    target=a.source/'target/x86_64-unknown-linux-gnu/release'
    if child.returncode==0:
        if a.component=='testloop':
            artifacts=[]
            for line in (a.output/'build.log').read_text().splitlines():
                try: row=json.loads(line)
                except ValueError: continue
                if row.get('reason')=='compiler-artifact' and row.get('executable') and row.get('profile',{}).get('test'):
                    artifacts.append(Path(row['executable']))
            require(len(artifacts)==1,'expected exactly one TestLoop test executable')
            binary=artifacts[0]
        else: binary=target/('neard' if a.component=='neard' else 'nft-receipt-exporter')
        report.update(executable=str(binary),executable_sha256=digest(binary),executable_bytes=binary.stat().st_size)
        args=['--list','--exact','nft_validation::bridge'] if a.component=='testloop' else ['--version']
        version=subprocess.run([str(binary),*args],capture_output=True,text=True,timeout=15)
        report['version_output']=version.stdout+version.stderr
        matches=('nft_validation::bridge: test' in version.stdout if a.component=='testloop' else VERSION in version.stdout and COMMIT in version.stdout)
        if version.returncode or not matches:
            report['build_status']='FAIL'; report['blockers'].append('built executable identity check failed')
    report['runtime_status']='NOT_RUN'  # A successful link/version is not Sandbox/TestLoop execution.
    save(a.output/'build.json',report); print(json.dumps(report,indent=2))
    return 0 if report['build_status']=='PASS' else 1

if __name__=='__main__': raise SystemExit(main())
