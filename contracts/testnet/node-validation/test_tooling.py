"""Harness tests only. Synthetic fixtures are never real-node evidence."""
import copy
import json
import subprocess
import io
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
from common import *
import run
import verify
import store_decode
import rebuild_node_store_exporter as exporter
import source_tree

class ToolingTests(unittest.TestCase):
    def graph_fixture(self):
        # A detached callback fails although the initial receipt succeeds.
        def outcome(i,children,status):
            return {'id':i,'outcome':{'receipt_ids':children,'tokens_burnt':'2','status':status}}
        result={'final_execution_status':'FINAL','transaction':{'hash':'tx'},
            'transaction_outcome':outcome('tx',['root'],{'SuccessReceiptId':'root'}),
            'receipts_outcome':[outcome('root',['child'],{'SuccessValue':''}),outcome('child',[],{'Failure':{'error':'GasExceeded'}})]}
        bodies={k:{'receipt_id':k,'receiver_id':MARKET,'receipt':{'Action':{'actions':[]}}} for k in ('root','child')}
        return result,bodies

    def test_graph_includes_detached_failure(self):
        result,bodies=self.graph_fixture()
        self.assertEqual(verify.graph(result,bodies)['tokens_burnt'],'6')
        self.assertIn('Failure',verify.outcomes(result)['child']['outcome']['status'])

    def test_graph_missing_body_is_inconclusive(self):
        result,bodies=self.graph_fixture(); del bodies['root']
        with self.assertRaises(Inconclusive): verify.graph(result,bodies)

    def test_graph_missing_outcome_fails(self):
        result,bodies=self.graph_fixture(); result['receipts_outcome'].pop()
        with self.assertRaises(CheckError): verify.graph(result,bodies)

    def test_graph_requires_final(self):
        result,bodies=self.graph_fixture(); result['final_execution_status']='EXECUTED'
        with self.assertRaises(CheckError): verify.graph(result,bodies)

    def test_graph_duplicate_and_orphan_rejected(self):
        for mode in ('duplicate','orphan','cycle'):
            result,bodies=self.graph_fixture()
            if mode=='duplicate': result['receipts_outcome'].append(result['receipts_outcome'][0])
            if mode=='orphan': result['receipts_outcome'][1]['id']='other'
            if mode=='cycle': result['receipts_outcome'][1]['outcome']['receipt_ids']=['root']
            with self.assertRaises(CheckError): verify.graph(result,bodies)

    def test_protocol_requires_all_runtime_parameters(self):
        ref=load(PARENT/'public/rpc-snapshot.json')['protocol_config']
        protocol_check(ref,ref)
        for path,value in [('storage_amount_per_byte','1'),('min_gas_purchase_price','0')]:
            bad=copy.deepcopy(ref); bad['runtime_config'][path]=value
            with self.assertRaises(CheckError): protocol_check(bad,ref)
        bad=copy.deepcopy(ref); bad['protocol_version']=87
        with self.assertRaises(CheckError): protocol_check(bad,ref)

    def test_no_rpc_or_home_mount_option(self):
        args=SimpleNamespace(window_seconds=3600,cases=['S01'])
        cmd=run.isolation_command(Path('/tmp/input'),Path('/tmp/output'),args)
        self.assertIn('--unshare-all',cmd); self.assertIn('--clearenv',cmd)
        self.assertNotIn('--share-net',cmd)
        self.assertFalse(any(x in ('/home','/root','/') for x in cmd))
        self.assertNotIn('--rpc',cmd)

    def test_manifest_local_identity_and_finite_expiry(self):
        template=load(PARENT/'public/manifest.template.json'); old=compact(template)
        identity={'chain_id':'nft-validation-local-test','genesis_hash':base58(b'\1'*32),'genesis_height':0,'timestamp_nanosec':'100'}
        m=manifest(template,identity,dict.fromkeys(ACCOUNTS,'public-only-fixture'),1000)
        self.assertEqual(m['flow']['expires_at_ns'],'1100')
        self.assertEqual(set(m['accounts'].values()),set(ACCOUNTS))
        self.assertEqual(compact(template),old)
        for bad in ('testnet','mainnet','other'):
            with self.assertRaises(CheckError): manifest(template,{**identity,'chain_id':bad},{},1000)
        with self.assertRaises(CheckError): manifest(template,identity,{},0)

    def sample_snapshot(self):
        return {'accounts':{a:{'state':{'values':[]},'account':{'storage_usage':100,'amount':'100','locked':'0','code_hash':'1'*32},'code_bytes':0} for a in ACCOUNTS}}

    def test_storage_record_overhead_and_code_are_separate(self):
        before=self.sample_snapshot(); after=copy.deepcopy(before)
        after['accounts'][NFT]['state']['values']=[{'key':b64(b'x'),'value':b64(b'yy')}]
        after['accounts'][NFT]['account']['storage_usage']+=43+10
        after['accounts'][NFT]['code_bytes']=10
        self.assertEqual(verify.storage_delta(before,after,NFT),{'bytes':53,'state_bytes':43,'code_bytes':10,'stake_yocto':str(53*BYTE_COST)})
        after['accounts'][NFT]['account']['storage_usage']-=40
        with self.assertRaises(CheckError): verify.storage_delta(before,after,NFT)

    def test_rollback_compares_values_not_only_size(self):
        before=self.sample_snapshot(); after=copy.deepcopy(before)
        before['accounts'][NFT]['state']['values']=[{'key':b64(b'x'),'value':b64(b'a')}]
        after['accounts'][NFT]['state']['values']=[{'key':b64(b'x'),'value':b64(b'b')}]
        with self.assertRaises(CheckError): verify.unchanged(before,after)

    def test_duplicate_state_keys_fail(self):
        snap=self.sample_snapshot()
        snap['accounts'][NFT]['state']['values']=[{'key':'eA==','value':'YQ=='}]*2
        with self.assertRaises(CheckError): state_map(snap,NFT)

    def test_balance_burn_refunds_and_duplicates(self):
        r,b=self.graph_fixture(); e={'result':r,'bodies':b}
        before=self.sample_snapshot(); after=copy.deepcopy(before)
        after['accounts'][OWNER]['account']['amount']='94'
        self.assertEqual(verify.balance(before,after,[e])['tokens_burnt'],'6')
        with self.assertRaises(CheckError): verify.balance(before,after,[e,e])
        after['accounts'][OWNER]['account']['amount']='93'
        with self.assertRaises(CheckError): verify.balance(before,after,[e])

    def test_pinned_vm_storage_envelope_decoded_offline_only(self):
        measurements=load(PARENT/'public/measurements.json')
        probe=load(PARENT/'public/measurement-input.json')
        # Frozen prior Rust-produced bytes verify the independent envelope and
        # totals decoder. This test does not claim receipt/runtime execution.
        rows={base64.b64decode(r['key']):base64.b64decode(r['value']) for r in measurements['marketplace_records']}
        # Bootstrap contains the canonical deployment bytes from that VM run.
        domain=rows[b'nmp:runtime:bootstrap:v1'][8:]
        root=verify.storage_body(rows,domain,b'\0')
        totals=verify.storage_totals(root[1:113])
        self.assertEqual(totals['funded'],71780000000000000000000)
        self.assertEqual(totals['staked'],38980000000000000000000)
        bad=dict(rows); bad[b'nmp:s:root:v1']=rows[b'nmp:s:root:v1'][:-1]+b'\0'
        with self.assertRaises(CheckError): verify.storage_body(bad,domain,b'\0')

    def test_signed_wire_matches_official_rust_encoder(self):
        helper=HERE/'target/x86_64-unknown-linux-gnu/debug/nft-local-signer'
        self.assertTrue(helper.is_file(),'build helper before harness tests')
        p=subprocess.Popen([str(helper),'--disposable-local-only'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        try:
            keys=json.loads(p.stdout.readline())['public_keys']
            request={'signer':OWNER,'receiver':NFT,'block_hash':base58(b'\2'*32),'nonce':'1',
                     'actions':[call('nft_approve',{'token_id':'wire-test','account_id':MARKET,'msg':None},4*10**12,188*BYTE_COST)]}
            p.stdin.write(compact(request)+'\n');p.stdin.flush()
            signed=json.loads(p.stdout.readline())
            verify.signed_transaction(signed['signed_tx_base64'],request,signed['hash'],keys[OWNER])
            for field in ('nonce','receiver'):
                bad=copy.deepcopy(request);bad[field]='2' if field=='nonce' else MARKET
                with self.assertRaises(CheckError): verify.signed_transaction(signed['signed_tx_base64'],bad,signed['hash'],keys[OWNER])
            bad=bytearray(base64.b64decode(signed['signed_tx_base64'])); bad[9]^=1
            with self.assertRaises(CheckError): verify.signed_transaction(b64(bad),request,signed['hash'],keys[OWNER])
            # The transaction hash excludes the signature: a shape/hash-only
            # verifier used to accept this corruption.
            bad=bytearray(base64.b64decode(signed['signed_tx_base64'])); bad[-1]^=1
            with self.assertRaisesRegex(CheckError,'invalid transaction signature'):
                verify.signed_transaction(b64(bad),request,signed['hash'],keys[OWNER])
            request['actions'][0]['args']['msg']='enable intake'
            p.stdin.write(compact(request)+'\n');p.stdin.flush()
            self.assertIn('error',json.loads(p.stdout.readline()))
        finally:
            p.stdin.close();p.stdout.close();p.wait(timeout=3)

    def test_unavailable_reports_all_twenty_without_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            run.unavailable(Path(tmp),['no compatible runtime'])
            report=load(Path(tmp)/'report.json')
            self.assertEqual([r['id'] for r in report['cases']],CASES)
            self.assertEqual({r['status'] for r in report['cases']},{'NOT_RUN'})

    def data_fixture(self, failure=False):
        def action(rid, receiver, inputs, outputs):
            return {'receipt_id':rid,'predecessor_id':MARKET,'receiver_id':receiver,
                'receipt':{'Action':{'actions':[],'input_data_ids':inputs,'output_data_receivers':outputs}}}
        result,bodies=self.graph_fixture()
        bodies['root']=action('root',NFT,[],[{'data_id':'data-id','receiver_id':MARKET}])
        bodies['child']=action('child',MARKET,['data-id'],[])
        status={'Failure':{'error':'gas'}} if failure else {'SuccessValue':b64(b'true')}
        result['receipts_outcome'][0]['outcome']['status']=status
        data={'receipt_id':'data-receipt','predecessor_id':NFT,'receiver_id':MARKET,
            'receipt':{'Data':{'data_id':'data-id','data':None if failure else b64(b'true')}}}
        return {'result':result,'bodies':bodies,'data_receipts':{'data-receipt':data},'unobserved_data_ids':[]}

    def test_data_success_and_failure_payloads(self):
        for failure in (False,True):
            self.assertEqual(verify.data_dependencies(self.data_fixture(failure)),{'dependencies':1,'data_receipts':1})

    def test_missing_data_cannot_be_hidden_by_empty_gap_list(self):
        e=self.data_fixture(); e['data_receipts']={}
        with self.assertRaises(Inconclusive): verify.data_dependencies(e)

    def test_data_rejects_payload_identity_and_duplicate_tampering(self):
        for field,value in [('predecessor_id',OWNER),('receiver_id',NFT),('receipt_id','other')]:
            e=self.data_fixture(); e['data_receipts']['data-receipt'][field]=value
            with self.assertRaises(CheckError): verify.data_dependencies(e)
        e=self.data_fixture(); e['data_receipts']['data-receipt']['receipt']['Data']['data']=b64(b'false')
        with self.assertRaises(CheckError): verify.data_dependencies(e)
        e=self.data_fixture(); e['data_receipts']['duplicate']=copy.deepcopy(e['data_receipts']['data-receipt'])
        e['data_receipts']['duplicate']['receipt_id']='duplicate'
        with self.assertRaises(CheckError): verify.data_dependencies(e)

    def test_data_requires_matching_consumer_and_preserves_failure(self):
        e=self.data_fixture(); e['bodies']['child']['receipt']['Action']['input_data_ids']=[]
        with self.assertRaises(CheckError): verify.data_dependencies(e)
        e=self.data_fixture(True); e['data_receipts']['data-receipt']['receipt']['Data']['data']=''
        with self.assertRaises(CheckError): verify.data_dependencies(e)

    def store_fixture(self):
        rid=b'\x03'*32; did=b'\x04'*32
        raw=string(NFT)+string(MARKET)+rid+b'\x01'+did+b'\x01'+integer(4,4)+b'true'
        body={'predecessor_id':NFT,'receiver_id':MARKET,'receipt_id':base58(rid),'priority':0,
              'receipt':{'Data':{'data_id':base58(did),'data':b64(b'true'),'is_promise_resume':False}}}
        # Two metadata rows: a real local processing location and a GC-only marker.
        metadata=integer(2,4)+b'\x00'+rid+b'\x00'+b'\x00'+b'\x05'*32+b'\x03'
        rows=[{'kind':'header','schema':1,'source_commit':COMMIT,'mode':'read-only','columns':['Receipts','ProcessedReceiptIds'],
               'genesis_height':0,'genesis_block_hash':base58(b'\x06'*32)},
            {'kind':'receipt','column':'Receipts','key_hex':rid.hex(),'borsh_hex':raw.hex(),'sha256':sha(raw).hex(),'receipt':body},
            {'kind':'processed','column':'ProcessedReceiptIds','key_hex':(b'\x06'*32+integer(1,8)).hex(),
             'borsh_hex':metadata.hex(),'sha256':sha(metadata).hex(),
             'metadata':[{'receipt_id':base58(rid),'source':'Local'},{'receipt_id':base58(b'\x05'*32),'source':'ReceiptToTxGc'}]},
            {'kind':'end','receipts':1,'processed_rows':1}]
        return rows

    def read_store_rows(self,rows,identity=None):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'records.jsonl'; path.write_text(''.join(json.dumps(r)+'\n' for r in rows))
            return store_decode.read_export(path,identity)

    def test_stored_borsh_is_independently_decoded_and_gc_is_not_delivery(self):
        rows=self.store_fixture(); receipts,locations=self.read_store_rows(rows)
        self.assertEqual(receipts[base58(b'\x03'*32)],rows[1]['receipt'])
        self.assertEqual(set(locations),{base58(b'\x03'*32)})
        self.assertEqual(locations[base58(b'\x03'*32)][0]['shard_id'],1)

    def test_export_cannot_be_replayed_against_another_genesis(self):
        rows=self.store_fixture()
        self.read_store_rows(rows,{'genesis_height':0,'genesis_hash':base58(b'\x06'*32)})
        with self.assertRaises(CheckError):
            self.read_store_rows(rows,{'genesis_height':0,'genesis_hash':base58(b'\x07'*32)})

    def test_export_rejects_truncation_conflicting_json_and_wrong_key(self):
        rows=self.store_fixture()
        for bad in (rows[:-1],rows+[rows[1]],rows[:2]+[rows[1]]+rows[2:]):
            with self.assertRaises(CheckError): self.read_store_rows(bad)
        for field,value in [('sha256','0'*64),('key_hex','00'*32)]:
            rows=self.store_fixture(); rows[1][field]=value
            with self.assertRaises(CheckError): self.read_store_rows(rows)
        rows=self.store_fixture(); rows[1]['receipt']['receiver_id']=NFT
        with self.assertRaises(CheckError): self.read_store_rows(rows)

    def test_receipt_decoder_rejects_trailing_truncated_and_unknown_variants(self):
        raw=bytes.fromhex(self.store_fixture()[1]['borsh_hex'])
        for bad in (raw+b'\0',raw[:-1],string(NFT)+string(MARKET)+b'\x03'*32+b'\x02'):
            with self.assertRaises(CheckError): store_decode.receipt(bad)

    def test_stored_deployment_view_is_hash_while_borsh_contains_code(self):
        code=b'\0asm-fixture-only'
        raw=(string(NFT)+string(NFT)+b'\x01'*32+b'\x00'+string(NFT)+b'\x00'+b'\x02'*32
             +integer(5000,16)+integer(0,4)+integer(0,4)+integer(1,4)+b'\x01'+integer(len(code),4)+code)
        decoded=store_decode.receipt(raw)
        self.assertEqual(decoded['receipt']['Action']['actions'],[{'DeployContract':{'code':b64(sha(code))}}])

    def test_receipt_decoder_matches_official_rust_borsh_and_rpc_views(self):
        helper=HERE/'target/x86_64-unknown-linux-gnu/debug/nft-local-signer'
        fixtures=json.loads(subprocess.check_output([str(helper),'--public-receipt-fixtures'],text=True,timeout=5))
        self.assertEqual(fixtures['evidence_class'],'synthetic-codec-fixtures')
        self.assertEqual(len(fixtures['receipts']),3)
        for row in fixtures['receipts']:
            self.assertEqual(store_decode.receipt(bytes.fromhex(row['borsh_hex'])),row['receipt'])

    def test_initial_receipt_actions_failure_phase_and_nonce_are_bound(self):
        result,bodies=self.graph_fixture()
        code=b'fixture'; key='ed25519:fixture'; before=self.sample_snapshot(); after=copy.deepcopy(before)
        before['block_hash']='block'
        for a in ACCOUNTS:
            before['accounts'][a]['keys']={'keys':[{'public_key':key,'access_key':{'nonce':0,'permission':'FullAccess'}}]}
            after['accounts'][a]['keys']={'keys':[{'public_key':key,'access_key':{'nonce':1 if a==NFT else 0,'permission':'FullAccess'}}]}
        bodies['root'].update(predecessor_id=NFT,receiver_id=NFT)
        bodies['root']['receipt']['Action']={'signer_id':NFT,'signer_public_key':key,'input_data_ids':[],
            'output_data_receivers':[],'actions':[{'DeployContract':{'code':b64(sha(code))}}]}
        e={'result':result,'bodies':bodies,'submitted':{'signer':NFT,'receiver':NFT,'block_hash':'block','nonce':'1',
            'actions':[{'kind':'deploy','code':b64(code)}]},'before':before,'after':after,'expected_success':True}
        verify.transaction_binding(e)
        bad=copy.deepcopy(e); bad['bodies']['root']['receipt']['Action']['actions'][0]['DeployContract']['code']=b64(code)
        with self.assertRaises(CheckError): verify.transaction_binding(bad)
        bad=copy.deepcopy(e); bad['after']['accounts'][NFT]['keys']['keys'][0]['access_key']['nonce']=0
        with self.assertRaises(CheckError): verify.transaction_binding(bad)
        e['expected_success']=False; e['expected_failure_action_index']=1
        e['result']['receipts_outcome'][0]['outcome']['status']={'Failure':{'ActionError':{'index':1,'kind':{'FunctionCallError':{}}}}}
        verify.transaction_binding(e)
        e['expected_failure_action_index']=2
        with self.assertRaises(CheckError): verify.transaction_binding(e)

    def test_source_tree_rejects_modified_or_extra_build_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); source=root/'lib.rs'; source.write_text('original')
            with patch.object(source_tree,'archive_files',return_value={'lib.rs':digest(source)}):
                source_tree.verify_tree(Path('synthetic-archive'),root)
                source.write_text('changed')
                with self.assertRaises(CheckError): source_tree.verify_tree(Path('synthetic-archive'),root)
                source.write_text('original'); (root/'build.rs').write_text('extra')
                with self.assertRaises(CheckError): source_tree.verify_tree(Path('synthetic-archive'),root)

    def test_bad_source_never_starts_a_build(self):
        import build_pinned
        with tempfile.TemporaryDirectory() as tmp:
            output=Path(tmp)/'report'
            args=['build_pinned','--source',tmp,'--archive',str(Path(tmp)/'missing.tar.gz'),'--output',str(output),'--build']
            with patch.object(sys,'argv',args), patch.object(build_pinned.subprocess,'Popen') as start, patch('builtins.print'):
                self.assertEqual(build_pinned.main(),2)
                start.assert_not_called()
            self.assertEqual(load(output/'build.json')['build_status'],'NOT_RUN')

    def test_export_augmentation_preserves_original_and_missing_is_inconclusive(self):
        e=self.data_fixture(); original=copy.deepcopy(e)
        data=e['data_receipts']; e['data_receipts']={}; before=copy.deepcopy(e)
        augmented=exporter.augment(e,data,{})
        self.assertEqual(e,before)
        verify.data_dependencies(augmented)
        with self.assertRaises(Inconclusive): verify.data_dependencies(exporter.augment(e,{},{}))
        conflict=copy.deepcopy(original['bodies']); conflict['root']['receiver_id']=OWNER
        with self.assertRaises(CheckError): exporter.augment(e,conflict,{})

    def test_exporter_mounts_only_read_only_databases_no_keys_or_home(self):
        cmd=exporter.export_command(Path('/tmp/stage'),Path('/tmp/case/data'),Path('/tmp/case/cold'))
        self.assertIn('--unshare-all',cmd)
        self.assertNotIn('--bind',cmd)
        self.assertNotIn('/keys',cmd)
        self.assertNotIn('/signers',cmd)
        self.assertNotIn('/home',cmd)

    def test_database_inventory_rejects_links_and_keys(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'CURRENT').write_text('MANIFEST-1\n'); (root/'LOCK').touch()
            self.assertEqual(len(exporter.inventory(root)),2)
            (root/'linked').symlink_to(root/'CURRENT')
            with self.assertRaises(CheckError): exporter.inventory(root)

    def test_testloop_pipe_handles_batched_lines_without_buffered_read_stall(self):
        from testloop import LoopNode
        node=LoopNode.__new__(LoopNode)
        node.receive_buffer=b''; node.log=io.StringIO()
        node.node=subprocess.Popen([sys.executable,'-c',
            'print("libtest preamble\\nNFT_VALIDATION {\\\"number\\\":1}\\nNFT_VALIDATION {\\\"number\\\":2}", flush=True)'],stdout=subprocess.PIPE)
        try:
            self.assertEqual(node.receive(2),{'number':1})
            self.assertEqual(node.receive(2),{'number':2})
        finally:
            node.node.wait(timeout=3); node.node.stdout.close()
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'validator_key.json').write_text('{}')
            with self.assertRaises(CheckError): exporter.inventory(root)

if __name__=='__main__': unittest.main()
