"""Sandbox recipes. Every variant starts a fresh node/genesis/manifest."""
import contextlib
import subprocess
import time
from common import *
from sandbox import Node
import verify

class Suite:
    def __init__(self,output,inputs,window_ns):
        self.output,self.inputs,self.window_ns=Path(output),Path(inputs),window_ns
        self.current=None; self.variant=0; self.nodes=[]; self.cal=None

    @contextlib.contextmanager
    def fresh(self,changes=None,window_ns=None):
        self.variant+=1
        if self.current.startswith('L'):
            from testloop import LoopNode
            implementation=LoopNode
        else: implementation=Node
        n=implementation(self.output/self.current/f'{self.variant:03d}',self.inputs,window_ns or self.window_ns,changes)
        self.nodes.append(n.output)
        try: yield n
        finally: n.close()

    def deploy(self,n,role,bad=False,gas=None,repeat=False):
        m=n.manifest; f=m['flow']; nft=role=='nft'; a=NFT if nft else MARKET
        args=({'mint_authority_id':OWNER,'marketplace_id':MARKET,'metadata':f['nft_metadata']} if nft else
              {'manifest':b64(deployment(m)),'admission_owner_id':OWNER})
        if bad:
            if nft: args['metadata']={**args['metadata'],'spec':'invalid'}
            else: args['manifest']=b64(b'\1'+deployment(m)[1:])
        init=call('new_testnet' if nft else 'bootstrap_with_admission_owner',args,gas or f['transaction_gas']['nft-deploy' if nft else 'marketplace-deploy'])
        actions=[{'kind':'deploy','code':b64((self.inputs/(role+'.wasm')).read_bytes())},init]
        if repeat: actions.append(init)
        return n.tx(role+'-deploy',a,a,actions,success=not(bad or repeat or gas==1),rollback=bad or repeat or gas==1,
                    failure_index=2 if repeat else 1 if bad or gas==1 else None)

    def step(self,n,stage,deposit=None,gas=None,success=True,rollback=False,state=None,dispatch=False):
        f=n.manifest['flow']; a=NFT if stage in ('mint','approve','revoke') else MARKET
        if stage=='fund': args={'funding_nonce':f['funding_nonce']}; method='fund_testnet_storage'; amount=f['funding_yocto']
        elif stage=='mint': args={'token_id':f['token_id'],'token_owner_id':OWNER,'token_metadata':f['token_metadata']}; method='nft_mint'; amount=f['mint_deposit_yocto']
        elif stage=='approve': args={'token_id':f['token_id'],'account_id':MARKET,'msg':None}; method='nft_approve'; amount=f['approval_deposit_yocto']
        elif stage=='revoke': args={'token_id':f['token_id'],'account_id':MARKET}; method='nft_revoke'; amount='1'
        elif stage=='create':
            args={'operation':b64(operation(n.manifest,n.approval)[0]),'storage_owner_id':OWNER,'reserve_amount':f['reserve_yocto'],'creation_bytes':f['creation_bytes'],'local_gas':f['local_gas']}; method='create_nft_admission'; amount='0'
        elif stage=='dispatch': args={'admission_id':b64(locator(n.manifest,n.approval))}; method='dispatch_nft_admission'; amount='0'
        elif stage=='release': args={'token_id':f['token_id'],'owner_id':OWNER,'approval_id':str(n.approval),'generation':'1'}; method='release_nft_admission'; amount='0'
        else: raise CheckError('unknown stage')
        e=n.tx(stage,OWNER,a,[call(method,args,gas or f['transaction_gas'].get(stage,CAP),amount if deposit is None else deposit)],success,rollback,state,dispatch)
        if stage=='approve' and success:
            n.approval=e['after']['accounts'][NFT]['views']['nft_token']['approved_account_ids'][MARKET]
            require(type(n.approval) is int,'approval must be actual numeric ID')
        return e

    def setup(self,n,until='create'):
        self.deploy(n,'nft'); self.deploy(n,'marketplace')
        for stage in ('fund','mint','approve','create'):
            if stage==until: return
            self.step(n,stage,state='ready' if stage=='create' else None)

    def calibrate(self):
        # Probe within the existing tool cap. Values remain observations until
        # exact-boundary cases confirm them; never written to public manifest.
        with self.fresh() as n:
            self.deploy(n,'nft'); self.deploy(n,'marketplace')
            measurements={}
            for stage,a in [('fund',MARKET),('mint',NFT),('approve',NFT),('create',MARKET)]:
                e=self.step(n,stage,gas=CAP,deposit=(16384*BYTE_COST if stage in ('mint','approve') else None),state='ready' if stage=='create' else None)
                measurements[stage]=verify.storage_delta(e['before'],e['after'],a)['state_bytes']
            e=self.step(n,'dispatch',gas=CAP,state='active',dispatch=True)
            token=verify.function_receipts(e,'nft_token')[0][0]
            obs=verify.outcomes(e['result'])
            measurements['result_bytes']=len(base64.b64decode(obs[token['receipt_id']]['outcome']['status']['SuccessValue']))
            measurements['source']='standalone-sandbox'
            self.cal=measurements
            save(self.output/'calibration.json',measurements)

    def S01(self):
        with self.fresh() as n:
            self.deploy(n,'nft'); self.deploy(n,'marketplace')
            snap=n.snapshot(); domain=deployment(n.manifest)
            require(state_map(snap,MARKET)[b'nmp:runtime:bootstrap:v1']==b'NMPBOOT1'+domain,'bootstrap commitment')
            owner=b'NMPAOWN1'+domain_hash(domain)+string(OWNER)
            require(state_map(snap,MARKET)[b'nmp:runtime:admission-owner:v1']==owner+sha(owner),'admission owner commitment')

    def S02(self):
        for role in ('nft','marketplace'):
            with self.fresh() as n: self.deploy(n,role,bad=True)

    def S03(self):
        for role in ('nft','marketplace'):
            with self.fresh() as n: self.deploy(n,role,gas=1)

    def S04(self):
        for role in ('nft','marketplace'):
            with self.fresh() as n: self.deploy(n,role,repeat=True)

    def need_cal(self):
        if self.cal is None: self.calibrate()

    def S05(self):
        self.need_cal()
        for stage in ('fund','mint','approve'):
            minimum=self.cal[stage]*BYTE_COST
            for minus in (0,1):
                with self.fresh() as n:
                    self.setup(n,stage)
                    e=self.step(n,stage,gas=CAP,deposit=minimum-minus,success=not minus,rollback=bool(minus))
                    if minus:
                        root=e['result']['transaction_outcome']['outcome']['receipt_ids'][0]
                        error=compact(verify.outcomes(e['result'])[root]['outcome']['status'])
                        require('GasExceeded' not in error and 'GasLimitExceeded' not in error,'storage boundary failed due to gas')

    def S06(self):
        self.need_cal()
        with self.fresh({'creation_bytes':str(self.cal['create']-1)}) as n:
            self.setup(n)
            e=self.step(n,'create',gas=CAP,success=False,rollback=True)
            require('Admission storage exceeds authorized budget' in compact(e['result']),'must fail after measured writes')

    def S07(self):
        with self.fresh() as n:
            self.setup(n,'dispatch')
            e=self.step(n,'dispatch',state='active',dispatch=True)
            for a in ACCOUNTS: require(verify.storage_delta(e['before'],e['after'],a)['bytes']==0,'callback grew storage')
            require(e['before']['accounts'][NFT]['views']['nft_token']==e['after']['accounts'][NFT]['views']['nft_token'],'NFT ownership/approval changed')

    def S08(self):
        for keys in [('first_call_gas',),('second_call_gas',),('first_call_gas','second_call_gas')]:
            with self.fresh(dict.fromkeys(keys,'1')) as n:
                self.setup(n,'dispatch')
                e=self.step(n,'dispatch',state='invalid',dispatch=True)
                ids=verify.dispatch_graph(e,n.manifest,n.approval); obs=verify.outcomes(e['result'])
                for key,method in [('first_call_gas','nft_token'),('second_call_gas','nft_is_approved')]:
                    require(('Failure' in obs[ids[method]]['outcome']['status'])==(key in keys),'child failure classification')

    def callback_trial(self,gas):
        with self.fresh({'callback_gas':str(gas)}) as n:
            self.setup(n,'dispatch')
            e=self.step(n,'dispatch',dispatch=True)
            ids=verify.dispatch_graph(e,n.manifest,n.approval); obs=verify.outcomes(e['result'])
            for method in ('nft_token','nft_is_approved'): require('SuccessValue' in obs[ids[method]]['outcome']['status'],'NFT call failed in callback case')
            ok='SuccessValue' in obs[ids['on_nft_verified']]['outcome']['status']
            verify.lifecycle(e['after'],n.manifest,n.approval,'active' if ok else 'dispatched')
            if not ok:
                error=compact(obs[ids['on_nft_verified']]['outcome']['status'])
                require('Gas' in error or 'gas' in error, 'callback failure must be OOG')
                retry=self.step(n,'dispatch',state='dispatched')
                verify.unchanged(retry['before'],retry['after'])
                require(not verify.function_receipts(retry,'nft_token'),'retry resent child graph')
                self.step(n,'release',success=False,rollback=True,state='dispatched')
            return ok

    def boundary(self,trial,upper):
        require(trial(upper),'upper budget failed')
        hi=upper; lo=upper//2
        while trial(lo): hi,lo=lo,lo//2
        # Resolution is explicit; never claim an exact raw-gas minimum.
        while hi-lo>100_000_000_000:
            mid=(hi+lo)//2
            if trial(mid): hi=mid
            else: lo=mid
        return {'failed_gas':str(lo),'passed_gas':str(hi),'resolution_gas':'100000000000'}

    def S09(self):
        require(not self.callback_trial(1),'one-gas callback unexpectedly succeeded')
        save(self.output/self.current/'callback-boundary.json',self.boundary(self.callback_trial,32*10**12))

    def S10(self):
        for stage in ('fund','mint','approve','create','dispatch'):
            def trial(gas):
                with self.fresh() as n:
                    self.setup(n,stage)
                    f=n.manifest['flow']
                    # step's expected receipt assertion is supplied from the
                    # observed receipt, then independently checked for rollback.
                    try:
                        e=self.step(n,stage,gas=gas)
                        return True
                    except CheckError as exc:
                        if not n.evidence or 'unexpected initial receipt outcome' not in str(exc): raise
                        e=n.evidence[-1]
                        verify.unchanged(e['before'],e['after'])
                        root=e['result']['transaction_outcome']['outcome']['receipt_ids'][0]
                        error=compact(verify.outcomes(e['result'])[root]['outcome']['status'])
                        require('Gas' in error or 'gas' in error,'unexpected non-gas failure')
                        require(not verify.function_receipts(e,'nft_token'),'failed dispatch emitted children')
                        e.update(expected_success=False,assert_rollback=True)
                        save(n.output/f'{n.seq:03d}-{stage}.json',e)
                        return False
            save(self.output/self.current/(stage+'-boundary.json'),self.boundary(trial,CAP))
        raise Inconclusive('Gas boundaries/rollback observed; uninstrumented traces cannot prove internal post-write OOG phase')

    def S11(self):
        self.need_cal(); size=self.cal['result_bytes']
        require(5<size<16384,'fixture result incompatible with bound sweep')
        for bound in (5,size-1,size,size+1):
            with self.fresh({'max_result_bytes':str(bound)}) as n:
                self.setup(n,'dispatch')
                self.step(n,'dispatch',state='invalid' if bound<size else 'active',dispatch=True)

    def S12(self):
        with self.fresh() as n:
            self.setup(n,'dispatch'); self.step(n,'revoke')
            self.step(n,'dispatch',state='invalid',dispatch=True)

    def S13(self):
        with self.fresh({'callback_gas':'1'}) as n:
            self.setup(n,'dispatch')
            self.step(n,'fund',success=False,rollback=True,state='ready')
            e=self.step(n,'create',state='ready'); verify.unchanged(e['before'],e['after'])
            self.step(n,'dispatch',state='dispatched',dispatch=True)
            e=self.step(n,'dispatch',state='dispatched'); verify.unchanged(e['before'],e['after'])
            require(not verify.function_receipts(e,'nft_token'),'duplicate dispatch graph')

    def S14(self):
        with self.fresh() as n:
            self.setup(n,'dispatch'); e=self.step(n,'release',state='released')
            require(verify.storage_delta(e['before'],e['after'],MARKET)['bytes']==0,'release grew storage')
            self.step(n,'dispatch',success=False,rollback=True,state='released')
        for gas,state in [('1','dispatched'),('32000000000000','active')]:
            with self.fresh({'callback_gas':gas}) as n:
                self.setup(n,'dispatch'); self.step(n,'dispatch',state=state,dispatch=True)
                self.step(n,'release',success=False,rollback=True,state=state)

    def S15(self):
        # The local fault proxy drops actual HTTP responses, not callback data.
        from fault_proxy import dropped_submission
        for stage in ('fund','dispatch'):
            for forwarded in (False,True):
                with self.fresh() as n:
                    self.setup(n,stage)
                    f=n.manifest['flow']
                    action=call('fund_testnet_storage' if stage=='fund' else 'dispatch_nft_admission',
                        {'funding_nonce':f['funding_nonce']} if stage=='fund' else {'admission_id':b64(locator(n.manifest,n.approval))},
                        f['transaction_gas'][stage],f['funding_yocto'] if stage=='fund' else '0')
                    journal=n.prepare(OWNER,MARKET,[action])
                    dropped_submission(n,journal,forwarded)
                    require(journal['state']=='UNKNOWN','client must retain UNKNOWN')
                    if forwarded:
                        e=n.collect(journal); e['expected_success']=True; save(n.output/'dropped-response.json',e)
                        if stage=='dispatch': verify.lifecycle(e['after'],n.manifest,n.approval,'active')
                    else:
                        try: n.collect(journal,timeout=3)
                        except Inconclusive: pass
                        else: raise CheckError('unforwarded transaction executed')
                        observed=n.snapshot()
                        verify.unchanged(journal['before'],observed)
                        save(n.output/'unforwarded-observation.json',{'journal_state':journal['state'],
                            'transaction_hash':journal['hash'],'before':journal['before'],'after':observed,
                            'claim':'UNKNOWN after bounded observation; not proof of permanent absence'})
                        require(journal['state']=='UNKNOWN','unknown cannot become absent/failure automatically')

    def S16(self):
        with self.fresh() as n:
            self.setup(n,'dispatch'); f=n.manifest['flow']
            j=n.send(OWNER,MARKET,[call('dispatch_nft_admission',{'admission_id':b64(locator(n.manifest,n.approval))},f['transaction_gas']['dispatch'])])
            # Stop retaining DB. Inspect a finalized checkpoint only if actually
            # captured; no claim that a wall-clock race guarantees pending work.
            checkpoint=n.snapshot(); save(n.output/'restart-checkpoint.json',checkpoint)
            n.stop(); n.start(); n.guard()
            e=n.collect(j); e['expected_success']=True; save(n.output/'restart.json',e)
            verify.lifecycle(e['after'],n.manifest,n.approval,'active')
            try: verify.lifecycle(checkpoint,n.manifest,n.approval,'dispatched')
            except (CheckError,KeyError): raise Inconclusive('restart succeeded, but no committed pending-callback checkpoint captured')
            raise Inconclusive('pending finalized state observed before stop; stopped-store proof that callback was still queued at shutdown is unavailable')

    def S17(self):
        self.need_cal()
        for extra in (0,1,2):
            with self.fresh() as n:
                self.deploy(n,'nft'); self.deploy(n,'marketplace')
                for stage in ('fund','mint','approve'):
                    deposit=(int(n.manifest['flow']['reserve_yocto'])+self.cal['fund']*BYTE_COST if stage=='fund' else self.cal[stage]*BYTE_COST)+extra
                    e=self.step(n,stage,gas=CAP,deposit=deposit)
                    if stage in ('mint','approve'):
                        refunds=[]
                        for r in e['bodies'].values():
                            body=verify.action_body(r)
                            if body and r['predecessor_id']==NFT and r['receiver_id']==OWNER:
                                refunds.extend(int(a['Transfer']['deposit']) for a in body['actions'] if 'Transfer' in a)
                        require(refunds==([2] if extra==2 else []),'NFT exact/excess refund branch')
                self.step(n,'create',state='ready'); self.step(n,'dispatch',state='active',dispatch=True)
                verify.balance(n.initial,n.snapshot(),n.evidence)

    def delayed_loop(self,expire):
        with self.fresh(window_ns=120*10**9 if expire else self.window_ns) as n:
            self.setup(n,'dispatch'); f=n.manifest['flow']
            j=n.send(OWNER,MARKET,[call('dispatch_nft_admission',{'admission_id':b64(locator(n.manifest,n.approval))},f['transaction_gas']['dispatch'])])
            gate=n.control('wait_parent_and_delay',hash=j['hash'])
            save(n.output/'delay-gate.json',gate)
            delay=n.control('advance',seconds=10)
            require(delay['withheld_endorsements']>0,'no genuine endorsement was withheld')
            save(n.output/'delay-observation.json',delay)
            pending=n.snapshot()
            verify.lifecycle(pending,n.manifest,n.approval,'dispatched')
            save(n.output/'pending-snapshot.json',pending)
            if expire:
                seconds=max(1,(int(f['expires_at_ns'])-int(pending['header']['timestamp_nanosec']))//10**9+2)
                require(seconds<=600,'finite delay exceeds control bound')
                n.control('advance',seconds=seconds)
                pending=n.snapshot()
                require(int(pending['header']['timestamp_nanosec'])>int(f['expires_at_ns']),'actual timestamp must cross expiry')
                verify.lifecycle(pending,n.manifest,n.approval,'dispatched')
                save(n.output/'expired-pending-snapshot.json',pending)
            n.control('resume')
            e=n.collect(j)
            e.update(expected_state='invalid' if expire else 'active',assert_dispatch=True,expected_success=True)
            save(n.output/'delayed-dispatch.json',e)
            verify.lifecycle(e['after'],n.manifest,n.approval,e['expected_state'])
            verify.dispatch_graph(e,n.manifest,n.approval)

    def L01(self): self.delayed_loop(False)
    def L02(self): self.delayed_loop(True)

    def L03(self):
        with self.fresh() as n:
            self.setup(n,'dispatch')
            e=self.step(n,'dispatch',state='active',dispatch=True)
            limitation=n.control('individual_data_delivery')
            save(n.output/'individual-delivery-capability.json',limitation)
            require('blocked' in limitation,'unreviewed per-receipt scheduler')
            raise Inconclusive('Authentic baseline captured; '+limitation['blocked']+'; no dependency/result substitution permitted')

    def run(self,selected):
        results=[{'id':case,'evidence_class':'standalone-sandbox' if case[0]=='S' else 'testloop-simulated-network','status':'NOT_RUN','reason':'not reached'} for case in CASES]
        save(self.output/'report.json',{'schema':1,'cases':results,'public_transactions':False})
        for row in results:
            case=row['id']
            if case not in selected: row['reason']='not selected'
            elif case[0]=='L' and not Path('/opt/nft-testloop').is_file():
                row.update(status='NOT_RUN',reason='Pinned TestLoop adapter executable is unavailable; no Sandbox/mock substitution')
            else:
                self.current=case; self.variant=0; self.nodes=[]
                print(case+' starting',flush=True)
                try:
                    getattr(self,case)()
                    row['status']='PASS'
                    row.pop('reason',None)
                except Inconclusive as exc: row.update(status='INCONCLUSIVE',reason=str(exc),incomplete_evidence=True)
                except (CheckError,KeyError,TypeError,OSError,subprocess.SubprocessError) as exc: row.update(status='FAIL',reason=str(exc))
                # A recipe cannot pass without a complete independently checked
                # graph, storage and balance evidence for every submitted step.
                checks=[]
                for node in self.nodes:
                    for path in sorted(node.glob('*.json')):
                        data=load(path)
                        if not isinstance(data,dict) or 'result' not in data or 'before' not in data: continue
                        try: checks.append({'file':str(path.relative_to(self.output)),'status':'PASS','checks':verify.verify_step(path,row['evidence_class'])})
                        except Inconclusive as exc:
                            checks.append({'file':str(path.relative_to(self.output)),'status':'INCONCLUSIVE','reason':str(exc)})
                            if row['status']=='PASS': row.update(status='INCONCLUSIVE',reason='incomplete independent evidence')
                        except (CheckError,KeyError,TypeError) as exc:
                            checks.append({'file':str(path.relative_to(self.output)),'status':'FAIL','reason':str(exc)})
                            row.update(status='FAIL',reason='independent verifier failed')
                row['evidence']=checks
                if row['status']=='PASS' and not checks:
                    row.update(status='FAIL',reason='case emitted no independently verifiable evidence')
                print(case+' '+row['status']+': '+row.get('reason','verified'),flush=True)
            save(self.output/'report.json',{'schema':1,'node_version':VERSION,'node_commit':COMMIT,'protocol':PROTOCOL,'cases':results,'public_transactions':False})
        return results
