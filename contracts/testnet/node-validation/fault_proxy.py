"""One-request loopback-only response-loss proxy, isolated alongside the node."""
import http.client
import http.server
import threading
from common import *
from sandbox import isolated

def dropped_submission(node,journal,forwarded):
    isolated(); node.guard()
    require(journal['state']=='PREPARED','fault proxy single submission')
    journal['state']='UNKNOWN'
    save(node.output/f'{node.seq:03d}-journal.json',journal)
    witness={'forward_requested':forwarded,'forwarded':False,'response_dropped':False}
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*args): pass
        def do_POST(self):
            raw=self.rfile.read(int(self.headers['Content-Length']))
            expected={'jsonrpc':'2.0','id':1,'method':'send_tx','params':{'signed_tx_base64':journal['signed_tx_base64'],'wait_until':'NONE'}}
            require(json.loads(raw)==expected,'fault proxy payload changed')
            if forwarded:
                c=http.client.HTTPConnection('127.0.0.1',node.port,timeout=8)
                try:
                    c.request('POST','/',raw,{'Content-Type':'application/json'})
                    witness['node_response']=json.loads(c.getresponse().read())
                    witness['forwarded']=True
                finally: c.close()
            self.close_connection=True
            witness['response_dropped']=True
    server=http.server.HTTPServer(('127.0.0.1',0),Handler)
    thread=threading.Thread(target=server.handle_request); thread.start()
    client=http.client.HTTPConnection('127.0.0.1',server.server_port,timeout=10)
    try:
        client.request('POST','/',compact({'jsonrpc':'2.0','id':1,'method':'send_tx','params':{'signed_tx_base64':journal['signed_tx_base64'],'wait_until':'NONE'}}),{'Content-Type':'application/json'})
        client.getresponse()
        raise CheckError('proxy failed to lose response')
    except http.client.RemoteDisconnected: pass
    finally:
        client.close(); thread.join(timeout=12); server.server_close()
    require(not thread.is_alive() and witness['response_dropped'] and witness['forwarded']==forwarded,'fault proxy did not exercise intended transport condition')
    save(node.output/'fault-proxy.json',witness)
