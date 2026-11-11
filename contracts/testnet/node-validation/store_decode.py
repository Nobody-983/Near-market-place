"""Independent bounded decoders for the pinned node's exported Borsh records.

Only ordinary Action/Data receipt forms needed by this suite are accepted.
These are consistency checks against retained bytes, not a public-chain proof.
"""
from common import *

class Reader:
    def __init__(self, raw): self.raw, self.pos = raw, 0
    def take(self,n):
        require(0<=n<=len(self.raw)-self.pos,'truncated Borsh record')
        result=self.raw[self.pos:self.pos+n]; self.pos+=n; return result
    def num(self,n): return int.from_bytes(self.take(n),'little')
    def blob(self): return self.take(self.num(4))
    def account(self):
        raw=self.blob(); require(2<=len(raw)<=64,'account byte length'); return raw.decode()
    def hash(self): return base58(self.take(32))
    def vector(self,read):
        size=self.num(4); require(size<=len(self.raw)-self.pos,'oversized Borsh vector')
        return [read() for _ in range(size)]
    def end(self): require(self.pos==len(self.raw),'trailing Borsh bytes')

def receipt(raw):
    r=Reader(raw)
    result={'predecessor_id':r.account(),'receiver_id':r.account(),'receipt_id':r.hash(),'priority':0}
    kind=r.num(1)
    if kind==0:
        signer=r.account(); require(r.num(1)==0,'non-Ed25519 receipt signer')
        key='ed25519:'+base58(r.take(32)); gas_price=str(r.num(16))
        outputs=r.vector(lambda:{'data_id':r.hash(),'receiver_id':r.account()})
        inputs=r.vector(r.hash)
        def action():
            tag=r.num(1)
            if tag==1: return {'DeployContract':{'code':b64(sha(r.blob()))}}
            if tag==2: return {'FunctionCall':{'method_name':r.blob().decode(),'args':b64(r.blob()),'gas':r.num(8),'deposit':str(r.num(16))}}
            if tag==3: return {'Transfer':{'deposit':str(r.num(16))}}
            raise CheckError('unexpected action in exported receipt')
        result['receipt']={'Action':{'signer_id':signer,'signer_public_key':key,'gas_price':gas_price,
            'output_data_receivers':outputs,'input_data_ids':inputs,'actions':r.vector(action),'is_promise_yield':False}}
    elif kind==1:
        data_id=r.hash(); present=r.num(1); require(present in (0,1),'invalid data option')
        result['receipt']={'Data':{'data_id':data_id,'data':b64(r.blob()) if present else None,'is_promise_resume':False}}
    else: raise CheckError('unexpected receipt variant at protocol 88')
    r.end(); return result

def processed(raw):
    r=Reader(raw)
    def row():
        require(r.num(1)==0,'unknown processed metadata version')
        rid=r.hash(); source=r.num(1)
        require(source<4,'unknown receipt source')
        return {'receipt_id':rid,'source':('Local','Delayed','Instant','ReceiptToTxGc')[source]}
    result=r.vector(row); r.end(); return result

def read_export(path, identity=None):
    receipts={}; locations={}; columns=set(); counts={'receipt':0,'processed':0}
    with Path(path).open() as f:
        header=json.loads(next(f))
        require({k:v for k,v in header.items() if k not in ('genesis_height','genesis_block_hash')}=={'kind':'header','schema':1,'source_commit':COMMIT,'mode':'read-only','columns':['Receipts','ProcessedReceiptIds']},'export header/pin mismatch')
        require(type(header['genesis_height']) is int and header['genesis_height']>=0,'export genesis height')
        require(len(unbase58(header['genesis_block_hash']))==32,'export genesis hash')
        if identity is not None:
            require(header['genesis_height']==identity['genesis_height'] and header['genesis_block_hash']==identity['genesis_hash'],'export belongs to a different local chain')
        ended=False
        for line in f:
            require(not ended,'records after export footer')
            row=json.loads(line); kind=row['kind']
            if kind=='end':
                require(row=={'kind':'end','receipts':counts['receipt'],'processed_rows':counts['processed']},'export count mismatch')
                ended=True; continue
            require(kind in counts,'unexpected export row')
            raw=bytes.fromhex(row['borsh_hex']); key=bytes.fromhex(row['key_hex'])
            require(sha(raw).hex()==row['sha256'],'exported Borsh checksum mismatch')
            require((row['column'],key) not in columns,'duplicate database row')
            columns.add((row['column'],key)); counts[kind]+=1
            if kind=='receipt':
                body=receipt(raw)
                require(row['column']=='Receipts' and body==row['receipt'],'exported receipt disagrees with Borsh')
                require(len(key)==32 and base58(key)==body['receipt_id'],'export receipt key mismatch')
                receipts[body['receipt_id']]=body
            else:
                rows=processed(raw)
                require(row['column']=='ProcessedReceiptIds' and len(key)==40 and rows==row['metadata'],'exported metadata disagrees with Borsh')
                for item in rows:
                    if item['source']=='ReceiptToTxGc': continue  # GC bookkeeping is not delivery.
                    locations.setdefault(item['receipt_id'],[]).append({'block_hash':base58(key[:32]),'shard_id':int.from_bytes(key[32:],'little'),'source':item['source']})
        require(ended,'truncated export: no footer')
    return receipts,locations
