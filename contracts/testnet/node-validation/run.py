#!/usr/bin/env python3
"""Launch validation in a new network/PID/mount namespace. No external RPC option.

Downloads/builds are deliberately separate. Only allowlisted public files enter
the namespace; keys and node DB live on its temporary filesystem and are erased.
"""
import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from common import *

def isolation_command(stage,output,args):
    command = ['bwrap','--unshare-all','--die-with-parent','--new-session',
            '--ro-bind','/usr','/usr','--symlink','usr/bin','/bin','--symlink','usr/lib','/lib','--symlink','usr/lib64','/lib64',
            '--proc','/proc','--dev','/dev','--tmpfs','/tmp','--dir','/run',
            '--ro-bind',str(stage),'/opt','--ro-bind',str(stage/'isolated'),'/run/nft-validation-isolated']
    if getattr(args,'runtime_dir',None):
        runtime=load(args.runtime_dir/'runtime.json')
        for relative in sorted(runtime['files']):
            command += ['--ro-bind', str(args.runtime_dir/'root'/relative), '/'+relative]
    return command + [
            '--bind',str(output),'/output','--clearenv','--setenv','PATH','/usr/bin:/bin','--setenv','PYTHONDONTWRITEBYTECODE','1',
            '--chdir','/opt/testnet/node-validation','--','/usr/bin/python3','/opt/testnet/node-validation/run.py','--inside',
            '--output','/output','--window-seconds',str(args.window_seconds),'--cases',*args.cases]

def unavailable(output,reasons):
    rows=[{'id':c,'status':'NOT_RUN','evidence_class':'standalone-sandbox' if c[0]=='S' else 'testloop-simulated-network',
           'implementation':'recipe_implemented_unverified' if c[0]=='S' else ('delivery_adapter_missing' if c=='L03' else 'adapter_implemented_unverified'),
           'reason':'; '.join(reasons) if c[0]=='S' else 'No compiled/executed pinned TestLoop evidence; L03 individual delivery control remains missing'} for c in CASES]
    save(output/'report.json',{'schema':1,'cases':rows,'blockers':reasons,'public_transactions':False})
    return 2

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--sandbox-bin',type=Path,help='explicit approved-version executable; no automatic download')
    p.add_argument('--sandbox-sha256',help='required expected digest of that binary')
    p.add_argument('--testloop-bin',type=Path,help='optional compiled pinned TestLoop bridge')
    p.add_argument('--testloop-sha256',help='required digest when a TestLoop bridge is supplied')
    p.add_argument('--signer-bin',type=Path,default=HERE/'target/x86_64-unknown-linux-gnu/debug/nft-local-signer')
    p.add_argument('--runtime-dir',type=Path,help='pinned GLIBC overlay produced by prepare_runtime.py; no host installation')
    p.add_argument('--output',type=Path,required=True,help='fresh public evidence directory; never a node home')
    p.add_argument('--window-seconds',type=int,required=True,help='explicit finite local fixture lifetime')
    p.add_argument('--cases',nargs='+',choices=CASES,default=CASES)
    p.add_argument('--inside',action='store_true',help=argparse.SUPPRESS)
    args=p.parse_args()
    require(1<=args.window_seconds<=86400,'expiry window must be 1..86400 seconds')
    if args.inside:
        from sandbox import isolated
        isolated()
        # Validate binary identity before init; stdout contains no private keys.
        if any(case.startswith('S') for case in args.cases):
            version=subprocess.run(['/opt/near-sandbox','--version'],capture_output=True,text=True,timeout=15)
            if version.returncode or VERSION not in version.stdout or COMMIT not in version.stdout:
                return unavailable(args.output,['node executable incompatible: '+version.stdout+version.stderr])
        from cases import Suite
        result=Suite(args.output,Path('/opt/inputs'),args.window_seconds*10**9).run(args.cases)
        return 0 if all(r['status']=='PASS' for r in result if r['id'] in args.cases) else 1
    require(not args.output.exists(),'output must be fresh; no overwrite of earlier evidence')
    args.output.mkdir(parents=True)
    args.output=args.output.resolve()
    reasons=[]
    if platform.system()!='Linux' or not shutil.which('bwrap'): reasons.append('Linux Bubblewrap is required')
    if any(case.startswith('S') for case in args.cases):
        if not args.sandbox_bin or not args.sandbox_bin.is_file(): reasons.append('exact Sandbox 2.14.0-rc.5 binary missing')
        elif not args.sandbox_sha256 or digest(args.sandbox_bin)!=args.sandbox_sha256: reasons.append('Sandbox binary SHA-256 missing/mismatched')
    if args.testloop_bin:
        if not args.testloop_bin.is_file() or not args.testloop_sha256 or digest(args.testloop_bin)!=args.testloop_sha256:
            reasons.append('TestLoop binary SHA-256 missing/mismatched')
    if not args.signer_bin.is_file(): reasons.append('build nft-local-signer with the standalone Cargo.lock')
    if reasons: return unavailable(args.output,reasons)
    runtime=None
    if args.runtime_dir:
        from prepare_runtime import validate_runtime
        args.runtime_dir=args.runtime_dir.resolve()
        runtime=validate_runtime(args.runtime_dir)
    public=PARENT/'public'
    before={name:digest(public/name) for name in ['manifest.template.json','rpc-snapshot.json','local-node-validation.plan.json']}
    from readiness import NFT as NFT_WASM, MARKET as MARKET_WASM, exports, NFT_EXPORTS, MARKET_EXPORTS
    for role,path,expected in [('nft',NFT_WASM,NFT_EXPORTS),('marketplace',MARKET_WASM,MARKET_EXPORTS)]:
        require(digest(path)==HASHES[role] and exports(path.read_bytes())==expected,'approved ordinary artifact mismatch')
    with tempfile.TemporaryDirectory(prefix='nft-validation-input-') as temp:
        stage=Path(temp); (stage/'testnet/node-validation').mkdir(parents=True); (stage/'inputs').mkdir()
        (stage/'isolated').write_text('disposable local namespace\n')
        for name in ['common.py','sandbox.py','testloop.py','cases.py','fault_proxy.py','verify.py','run.py']:
            shutil.copyfile(HERE/name,stage/'testnet/node-validation'/name)
        for name in ['flow.py','readiness.py']: shutil.copyfile(PARENT/name,stage/'testnet'/name)
        for name in before: shutil.copyfile(public/name,stage/'inputs'/name)
        for role,path in [('nft',NFT_WASM),('marketplace',MARKET_WASM)]: shutil.copyfile(path,stage/'inputs'/(role+'.wasm'))
        if any(case.startswith('S') for case in args.cases):
            shutil.copyfile(args.sandbox_bin,stage/'near-sandbox'); (stage/'near-sandbox').chmod(0o555)
            require(digest(stage/'near-sandbox')==args.sandbox_sha256,'staged node digest mismatch')
        if args.testloop_bin:
            shutil.copyfile(args.testloop_bin,stage/'nft-testloop'); (stage/'nft-testloop').chmod(0o555)
            require(digest(stage/'nft-testloop')==args.testloop_sha256,'staged TestLoop digest mismatch')
        shutil.copyfile(args.signer_bin,stage/'nft-local-signer'); (stage/'nft-local-signer').chmod(0o555)
        save(args.output/'launcher.json',{'node_sha256':args.sandbox_sha256,'signer_sha256':digest(args.signer_bin),'cargo_lock_sha256':digest(HERE/'Cargo.lock'),
            'testloop_sha256':args.testloop_sha256,'testloop_overlay_sha256':digest(HERE/'testloop/nft_validation.rs'),
            'public_inputs_sha256':before,'runtime_overlay':runtime,'tooling_sha256':{p.name:digest(p) for p in HERE.glob('*.py')},'isolation':'bubblewrap unshare-all; no home/repo mounts; network loopback only'})
        result=subprocess.run(isolation_command(stage,args.output,args),check=False)
    require(before=={name:digest(public/name) for name in before},'public preparation inputs changed')
    if not (args.output/'report.json').exists():
        return unavailable(args.output,['isolation/node launcher exited before a case report; exit '+str(result.returncode)])
    return result.returncode

if __name__=='__main__':
    raise SystemExit(main())
