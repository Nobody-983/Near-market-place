"""Python controller for the pinned nearcore TestLoop bridge, not a Sandbox node."""
import os
import selectors
import subprocess
import tempfile
import time
import uuid
from common import *
from sandbox import Node, RPCError, isolated

class LoopNode(Node):
    def __init__(self, output, inputs, window_ns, changes=None):
        isolated()
        self.output, self.inputs = Path(output), Path(inputs)
        self.output.mkdir(parents=True, exist_ok=False)
        self.temp = tempfile.TemporaryDirectory(prefix='nft-testloop-')
        self.home = Path(self.temp.name)
        self.node = None; self.signer = None
        self.receive_buffer = b''
        self.rpc_counter = 0; self.seq = 0; self.evidence = []
        self.identity = None
        self.reference = load(inputs/'rpc-snapshot.json')['protocol_config']
        self.chain = 'nft-validation-local-' + uuid.uuid4().hex
        self.node_digest = digest('/opt/nft-testloop')
        self.log = (self.output/'testloop.log').open('w')
        try:
            self.signer = subprocess.Popen(['/opt/nft-local-signer','--disposable-local-only'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1)
            self.keys = json.loads(self.signer.stdout.readline())['public_keys']
            require(set(self.keys)==set(ACCOUNTS),'generated account allowlist')
            self.node = subprocess.Popen(['/opt/nft-testloop','--exact','nft_validation::bridge','--nocapture'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,bufsize=0)
            self.node.stdin.write((compact({'chain_id':self.chain,'public_keys':self.keys})+'\n').encode()); self.node.stdin.flush()
            ready = self.receive(120)
            require(ready['source_commit']==COMMIT and ready['evidence_class']=='testloop-simulated-network','TestLoop provenance')
            require(ready['validators']==4 and ready['nft_shard']!=ready['marketplace_shard'],'TestLoop topology')
            save(self.output/'testloop-topology.json',ready)
            status = self.rpc('status',[])
            genesis = self.rpc('block',{'block_id':0})['header']
            self.identity = {'chain_id':self.chain,'genesis_hash':genesis['hash'],'genesis_height':0,
                             'timestamp_nanosec':self.rpc('block',{'finality':'final'})['header']['timestamp_nanosec']}
            self.guard()
            self.manifest = manifest(load(inputs/'manifest.template.json'),self.identity,self.keys,window_ns,changes)
            save(self.output/'local-manifest.json',self.manifest)
            save(self.output/'environment.json',{'status':status,'identity':self.identity,'binary_sha256':self.node_digest,'evidence_class':'testloop-simulated-network'})
            self.approval = None
            self.initial = self.snapshot()
        except BaseException:
            self.close(); raise

    def receive(self, timeout=60):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            if b'\n' in self.receive_buffer:
                raw,self.receive_buffer=self.receive_buffer.split(b'\n',1)
                line=raw.decode()
                if line.startswith('NFT_VALIDATION '): return json.loads(line[len('NFT_VALIDATION '):])
                self.log.write(line+'\n'); self.log.flush()
                continue
            # select guards a dead/stalled native process without driving the
            # simulated clock or manufacturing any runtime observation.
            with selectors.DefaultSelector() as selector:
                selector.register(self.node.stdout,selectors.EVENT_READ)
                if not selector.select(max(0,deadline-time.monotonic())):
                    raise Inconclusive('TestLoop bridge wall-clock timeout')
            chunk=os.read(self.node.stdout.fileno(),65536)
            if not chunk: raise CheckError('TestLoop exited; see testloop.log')
            self.receive_buffer+=chunk
            require(len(self.receive_buffer)<=32*1024*1024,'TestLoop response too large')
        raise Inconclusive('TestLoop bridge timeout')

    def control(self,op,**args):
        isolated()
        require(self.node.poll() is None,'TestLoop process exited')
        self.node.stdin.write((compact({'op':op,**args})+'\n').encode()); self.node.stdin.flush()
        result=self.receive()
        return result

    def rpc(self,method,params):
        require(method in {'status','block','chunk','query','EXPERIMENTAL_protocol_config','EXPERIMENTAL_receipt','EXPERIMENTAL_tx_status','send_tx'},'TestLoop RPC allowlist')
        if method=='query': require(params['account_id'] in ACCOUNTS,'TestLoop account allowlist')
        request={'jsonrpc':'2.0','id':self.rpc_counter,'method':method,'params':params}
        self.rpc_counter+=1
        result=self.control('rpc',request=request)
        save(self.output/'rpc'/f'{self.rpc_counter:06d}.json',{'request':request,'response':result,'transport':'TestLoop in-process router'})
        if 'error' in result: raise RPCError(result)
        return result['result']

    def guard(self):
        isolated()
        require(digest('/opt/nft-testloop')==self.node_digest,'TestLoop binary changed')
        require(Path(f'/proc/{self.node.pid}/exe').resolve()==Path('/opt/nft-testloop'),'TestLoop executable mismatch')
        status=self.rpc('status',[])
        require(status['chain_id']==self.chain and status['genesis_hash']==self.identity['genesis_hash'],'TestLoop local genesis changed')
        require(status['protocol_version']==PROTOCOL,'TestLoop protocol mismatch')
        protocol_check(self.rpc('EXPERIMENTAL_protocol_config',{'finality':'final'}),self.reference)
        for role in HASHES: require(digest(self.inputs/(role+'.wasm'))==HASHES[role],'artifact changed')

    def collect(self,journal,timeout=60):
        evidence=super().collect(journal,timeout)
        evidence['evidence_class']='testloop-simulated-network'
        return evidence

    def stop(self):
        if self.node and self.node.poll() is None:
            try:
                self.node.stdin.write(b'{"op":"stop"}\n'); self.node.stdin.flush()
                self.node.wait(timeout=10)
            except (BrokenPipeError,subprocess.TimeoutExpired):
                self.node.kill(); self.node.wait()
