"""Owned local node and raw evidence collection. No configurable RPC endpoint."""
import http.client
import os
import signal
import shutil
import socket
import subprocess
import tempfile
import time
import uuid
from common import *
import verify

class RPCError(CheckError):
    def __init__(self, response):
        self.response = response
        super().__init__(compact(response))

def isolated():
    require(Path('/run/nft-validation-isolated').is_file(), 'use run.py; direct execution prohibited')
    require({name for _,name in socket.if_nameindex()} == {'lo'}, 'non-loopback interface in namespace')
    require(not Path('/home/acknowlege').exists() and not Path('/root').exists(), 'home directories exposed')

class Node:
    def __init__(self, output, inputs, window_ns, changes=None):
        isolated()
        self.output, self.inputs = Path(output), Path(inputs)
        self.output.mkdir(parents=True, exist_ok=False)
        self.temp = tempfile.TemporaryDirectory(prefix='nft-local-')
        self.home = Path(self.temp.name)
        self.node = None
        self.signer = None
        self.rpc_counter = 0
        self.seq = 0
        self.evidence = []
        self.identity = None
        self.node_digest = digest('/opt/near-sandbox')
        self.reference = load(inputs / 'rpc-snapshot.json')['protocol_config']
        self.chain = 'nft-validation-local-' + uuid.uuid4().hex
        self.port = 3030  # Unique network namespace; serial cases, owned process.
        self.log = (self.output / 'node.log').open('w')
        try:
            self.signer = subprocess.Popen(['/opt/nft-local-signer','--disposable-local-only'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
            self.keys = json.loads(self.signer.stdout.readline())['public_keys']
            require(set(self.keys) == set(ACCOUNTS), 'unexpected generated accounts')
            subprocess.run(['/opt/near-sandbox','--home',str(self.home),'init','--chain-id',self.chain,'--account-id','local-validator'],check=True,stdout=self.log,stderr=self.log,timeout=30)
            genesis = load(self.home/'genesis.json')
            require(genesis['chain_id']==self.chain, 'init chain changed')
            genesis['protocol_version'] = PROTOCOL
            genesis['gas_limit'] = self.reference['gas_limit']
            # Preserve normal protocol charging. This changes only local network
            # economics/topology inputs, not runtime costs or safety checks.
            genesis['min_gas_price'] = self.reference['min_gas_price']
            for account in ACCOUNTS:
                genesis['records'].extend([
                    {'Account': {'account_id':account,'account':{'amount':str(10*10**24),'locked':'0','code_hash':'1'*32,'storage_usage':0,'version':'V1'}}},
                    {'AccessKey': {'account_id':account,'public_key':self.keys[account],'access_key':{'nonce':0,'permission':'FullAccess'}}},
                ])
            genesis['total_supply'] = str(int(genesis['total_supply']) + 30*10**24)
            save(self.home/'genesis.json',genesis)
            config = load(self.home/'config.json')
            config['archive'] = True
            config['save_tx_outcomes'] = True
            config['save_receipt_to_tx'] = True
            # This nearcore revision requires a cold store for local archival
            # mode. Merely setting archive=true creates an inconsistent DB kind.
            config['store']['col_state_cache_size'] = 64 * 1024 * 1024
            config['store']['col_flat_state_cache_size'] = 16 * 1024 * 1024
            config['store']['trie_cache']['default_max_bytes'] = 64 * 1024 * 1024
            config['store']['view_trie_cache']['default_max_bytes'] = 16 * 1024 * 1024
            config['cold_store'] = {**config['store'], 'path': 'cold-data'}
            config['cold_store']['rocksdb'] = {'use_direct_io_for_flush_and_compaction': False}
            config['save_trie_changes'] = True
            config['tracked_shards_config'] = 'AllShards'
            config['network']['addr'] = '127.0.0.1:24567'
            config['network']['boot_nodes'] = ''
            config['network']['trusted_stun_servers'] = []
            config['telemetry'] = {'endpoints': []}
            config['rpc']['addr'] = '127.0.0.1:3030'
            config['rpc']['prometheus_addr'] = None
            save(self.home/'config.json',config)
            save(self.output/'genesis.json',genesis)
            save(self.output/'config.json',config)
            save(self.output/'public-keys.json',self.keys)
            self.start()
            status = self.rpc('status',[])
            require(status['version']['version']==VERSION and status['version']['commit']==COMMIT, 'node exact version/commit mismatch')
            require(status['version']['rustc_version']=='1.95.0', 'node compiler mismatch')
            genesis_header = self.rpc('block',{'block_id':genesis['genesis_height']})['header']
            require(status['genesis_hash']==genesis_header['hash'], 'genesis identity mismatch')
            self.identity = {'chain_id':self.chain, 'genesis_hash':genesis_header['hash'], 'genesis_height':genesis['genesis_height'],
                             'timestamp_nanosec': self.rpc('block',{'finality':'final'})['header']['timestamp_nanosec']}
            self.guard()
            self.manifest = manifest(load(inputs/'manifest.template.json'),self.identity,self.keys,window_ns,changes)
            save(self.output/'local-manifest.json',self.manifest)
            save(self.output/'environment.json',{'status':status,'identity':self.identity,'binary_sha256':self.node_digest,'evidence_class':'standalone-sandbox'})
            self.approval = None
            self.initial = self.snapshot()
            for a in ACCOUNTS:
                require(self.initial['accounts'][a]['account']['code_hash']=='1'*32 and not state_map(self.initial,a), 'genesis contracts must be empty')
        except BaseException:
            self.close()
            raise

    def start(self):
        self.node = subprocess.Popen(['/opt/near-sandbox','--home',str(self.home),'run'],stdout=self.log,stderr=self.log)
        deadline = time.monotonic()+45
        while time.monotonic()<deadline:
            require(self.node.poll() is None, 'Sandbox exited; see node.log')
            try:
                s = self.rpc('status',[])
                if not s['sync_info']['syncing']: return
            except (OSError,RPCError): pass
            time.sleep(.2)
        raise Inconclusive('Sandbox startup timeout')

    def stop(self):
        if self.node and self.node.poll() is None:
            self.node.send_signal(signal.SIGINT)
            try: self.node.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.node.kill(); self.node.wait()

    def close(self):
        self.stop()
        # Chain DB is public state, unlike sibling node/validator key files.
        # Preserve only the stopped DB when RPC could not expose all receipts.
        if any(e.get('unobserved_data_ids') or
               {rid for row in verify.outcomes(e['result']).values() for rid in row['outcome']['receipt_ids']}-e['bodies'].keys()
               for e in self.evidence) and (self.home/'data').is_dir():
            shutil.copytree(self.home/'data',self.output/'stopped-node-data',symlinks=False)
            if (self.home/'cold-data').is_dir():
                shutil.copytree(self.home/'cold-data',self.output/'stopped-node-cold-data',symlinks=False)
            save(self.output/'stopped-store.json',{'node_returncode':self.node.returncode,
                'chain_id':self.chain,'binary_sha256':self.node_digest,
                'contents':'stopped hot/cold databases only; no node or validator keys'})
        if self.signer:
            if self.signer.stdin: self.signer.stdin.close()
            try: self.signer.wait(timeout=3)
            except subprocess.TimeoutExpired: self.signer.kill(); self.signer.wait()
            if self.signer.stdout: self.signer.stdout.close()
        self.log.close()
        self.temp.cleanup()  # Only the freshly created owned node home, including disposable keys.

    def rpc(self, method, params):
        isolated()
        require(self.node is not None and self.node.poll() is None, 'RPC requires owned live node')
        require(method in {'status','block','chunk','query','EXPERIMENTAL_protocol_config','EXPERIMENTAL_receipt','EXPERIMENTAL_tx_status','send_tx'}, 'RPC method allowlist')
        if method == 'query': require(params['account_id'] in ACCOUNTS, 'RPC account allowlist')
        conn = http.client.HTTPConnection('127.0.0.1',self.port,timeout=8)
        request={'jsonrpc':'2.0','id':self.rpc_counter,'method':method,'params':params}
        self.rpc_counter += 1
        try:
            conn.connect()
            require(conn.sock.getpeername()==('127.0.0.1',self.port), 'RPC socket identity')
            conn.request('POST','/',compact(request),{'Content-Type':'application/json'})
            response=conn.getresponse()
            require(response.status==200, 'RPC HTTP status/redirect rejected')
            raw=response.read(32*1024*1024+1)
            require(len(raw)<=32*1024*1024,'RPC response too large')
            data=json.loads(raw)
            save(self.output/'rpc'/f'{self.rpc_counter:06d}.json',{'request':request,'response':data})
            if 'error' in data: raise RPCError(data)
            return data['result']
        finally: conn.close()

    def guard(self):
        isolated()
        require(digest('/opt/near-sandbox')==self.node_digest,'node binary changed')
        require(Path(f'/proc/{self.node.pid}/exe').resolve()==Path('/opt/near-sandbox'),'unexpected process executable')
        status=self.rpc('status',[])
        require(status['chain_id']==self.chain and status['genesis_hash']==self.identity['genesis_hash'],'network/genesis changed')
        require(status['version']['version']==VERSION and status['version']['commit']==COMMIT,'version changed')
        require(status['protocol_version']==PROTOCOL,'protocol changed')
        protocol_check(self.rpc('EXPERIMENTAL_protocol_config',{'finality':'final'}),self.reference)
        for role in HASHES: require(digest(self.inputs/(role+'.wasm'))==HASHES[role], 'artifact changed')

    def query(self,account,block,kind,**kw):
        return self.rpc('query',{'request_type':kind,'account_id':account,'block_id':block,**kw})

    def view(self,account,block,method,args):
        r=self.query(account,block,'call_function',method_name=method,args_base64=b64(compact(args).encode()))
        require('result' in r, 'view failed: '+method)
        require(r['block_hash']==block,'mixed view block')
        return json.loads(bytes(r['result']))

    def snapshot(self):
        header=self.rpc('block',{'finality':'final'})['header']
        block=header['hash']; data={'block_hash':block,'header':header,'accounts':{}}
        for a in ACCOUNTS:
            row={k:self.query(a,block,kind,**extra) for k,kind,extra in [('account','view_account',{}),('keys','view_access_key_list',{}),('state','view_state',{'prefix_base64':''})]}
            require(all(row[k]['block_hash']==block for k in row),'mixed snapshot block')
            require([(k['public_key'],k['access_key']['permission']) for k in row['keys']['keys']]==[(self.keys[a],'FullAccess')],'local key inventory changed')
            row['code_bytes']=0; row['views']={}
            if row['account']['code_hash']!='1'*32:
                role='nft' if a==NFT else 'marketplace'
                require(unbase58(row['account']['code_hash']).hex()==HASHES[role],'unexpected executing code')
                code=self.query(a,block,'view_code')
                require(sha(base64.b64decode(code['code_base64'])).hex()==HASHES[role],'code bytes mismatch')
                row['code_bytes']=len(base64.b64decode(code['code_base64']))
                if a==MARKET:
                    row['views']['storage_status']=self.view(a,block,'storage_status',{})
                    require(row['views']['storage_status']['trading_enabled'] is False,'trading enabled')
                else:
                    f=self.manifest['flow']
                    for method,args in [('marketplace_id',{}),('contract_owner_id',{}),('testnet_mint_authority',{}),('nft_token',{'token_id':f['token_id']}),('nft_total_supply',{}),('nft_tokens_for_owner',{'account_id':OWNER})]:
                        row['views'][method]=self.view(a,block,method,args)
                    require(row['views']['marketplace_id']==MARKET and row['views']['contract_owner_id']==OWNER and row['views']['testnet_mint_authority']==OWNER,'NFT bindings')
            data['accounts'][a]=row
        return data

    def prepare(self, signer, receiver, actions):
        self.guard()
        before=self.snapshot()
        key=before['accounts'][signer]['keys']['keys'][0]['access_key']
        req={'signer':signer,'receiver':receiver,'actions':actions,'nonce':str(int(key['nonce'])+1),'block_hash':before['block_hash']}
        self.signer.stdin.write(compact(req)+'\n'); self.signer.stdin.flush()
        signed=json.loads(self.signer.stdout.readline())
        require('error' not in signed, 'signer rejected action: '+str(signed.get('error')))
        verify.signed_transaction(signed['signed_tx_base64'],req,signed['hash'],self.keys[signer])
        self.seq+=1
        journal={'state':'PREPARED','request':req,**signed,'before':before}
        save(self.output/f'{self.seq:03d}-journal.json',journal) # durable hash BEFORE send
        return journal

    def submit(self,journal):
        require(journal['state']=='PREPARED','submission is single-use')
        self.guard()
        journal['state']='UNKNOWN'
        save(self.output/f'{self.seq:03d}-journal.json',journal)
        try:
            result=self.rpc('send_tx',{'signed_tx_base64':journal['signed_tx_base64'],'wait_until':'NONE'})
            journal['submit_response']=result
        except (RPCError,OSError) as e:
            journal['submit_error']=str(e)  # Do not re-sign or re-submit.
        save(self.output/f'{self.seq:03d}-journal.json',journal)

    def collect(self,journal,timeout=60):
        deadline=time.monotonic()+timeout; result=None
        while time.monotonic()<deadline:
            try:
                r=self.rpc('EXPERIMENTAL_tx_status',{'tx_hash':journal['hash'],'sender_account_id':journal['request']['signer'],'wait_until':'FINAL'})
                if r.get('final_execution_status')=='FINAL': result=r; break
            except (OSError,RPCError): pass
            time.sleep(.25)
        if result is None: raise Inconclusive('UNKNOWN: no finalized outcome; journal retained; no automatic retry')
        bodies={r['receipt_id']:r for r in result.get('receipts',[])}
        required={rid for row in verify.outcomes(result).values() for rid in row['outcome']['receipt_ids']}
        for rid in required-bodies.keys():
            try: bodies[rid]=self.rpc('EXPERIMENTAL_receipt',{'receipt_id':rid})
            except RPCError: pass
        # Scan authentic chunk receipt bodies over the transaction's execution
        # interval. Never synthesize the self-signed initial receipt.
        end=self.rpc('block',{'finality':'final'})['header']['height']
        for height in range(int(journal['before']['header']['height'])+1,int(end)+1):
            block=self.rpc('block',{'block_id':height})
            for chunk in block['chunks']:
                data=self.rpc('chunk',{'chunk_id':chunk['chunk_hash']})
                for r in data.get('receipts',[]): bodies[r['receipt_id']]=r
        # Outcome edges do not enumerate every DataReceipt. Keep separately
        # captured authentic data deliveries instead of discarding them.
        data_ids={d['data_id'] for rid,r in bodies.items() if rid in required and verify.action_body(r)
                  for d in verify.action_body(r).get('output_data_receivers',[])}
        data_receipts={k:v for k,v in bodies.items() if 'Data' in v['receipt'] and v['receipt']['Data']['data_id'] in data_ids}
        data_seen={v['receipt']['Data']['data_id'] for v in data_receipts.values()}
        bodies={k:v for k,v in bodies.items() if k in required}
        evidence={'result':result,'bodies':bodies,'before':journal['before'],'after':self.snapshot(),'submitted':journal['request'],
                  'manifest':self.manifest,'approval':self.approval,'evidence_class':'standalone-sandbox',
                  'signed_tx_base64':journal['signed_tx_base64'],'transaction_hash':journal['hash'],
                  'data_receipts':data_receipts,'unobserved_data_ids':sorted(data_ids-data_seen)}
        self.evidence.append(evidence)
        journal['state']='FINAL'; save(self.output/f'{self.seq:03d}-journal.json',journal)
        return evidence

    def tx(self,label,signer,receiver,actions,success=True,rollback=False,state=None,dispatch=False,failure_index=None):
        e=self.collect(self.send(signer,receiver,actions))
        e.update(assert_rollback=rollback,expected_state=state,assert_dispatch=dispatch,
                 expected_success=success,expected_failure_action_index=failure_index)
        path=self.output/f'{self.seq:03d}-{label}.json'
        save(path,e)
        obs=verify.outcomes(e['result'])
        root=e['result']['transaction_outcome']['outcome']['receipt_ids'][0]
        actual='Failure' not in obs[root]['outcome']['status']
        require(actual==success, f'{label}: unexpected initial receipt outcome: '+compact(obs[root]['outcome']['status']))
        if rollback: verify.unchanged(e['before'],e['after'])
        if state: verify.lifecycle(e['after'],self.manifest,self.approval,state)
        if dispatch: verify.dispatch_graph(e,self.manifest,self.approval)
        return e

    def send(self,signer,receiver,actions):
        journal=self.prepare(signer,receiver,actions); self.submit(journal); return journal
