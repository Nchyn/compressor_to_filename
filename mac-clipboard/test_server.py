import importlib.util, threading, tempfile, urllib.request, urllib.error, json
from pathlib import Path
spec=importlib.util.spec_from_file_location('bridge',Path(__file__).with_name('server.py'))
bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
 bridge.CACHE=Path(directory)
 server=bridge.ThreadingHTTPServer(('127.0.0.1',0),bridge.Handler)
 thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
 base=f'http://127.0.0.1:{server.server_port}'
 def request(path,body=b'{}',token=True,origin=None,kind='application/json'):
  req=urllib.request.Request(base+path,data=body,headers={'Origin':origin or base,'Authorization':'Bearer '+(bridge.TOKEN if token else 'bad'),'Content-Type':kind})
  try:
   with urllib.request.urlopen(req) as r:return r.status,json.load(r)
  except urllib.error.HTTPError as e:return e.code,json.load(e)
 with urllib.request.urlopen(base+'/'+bridge.RAIL) as response:
  page=response.read().decode();csp=response.headers['Content-Security-Policy']
  assert 'clientDot' in page and '<script nonce="' in page
  assert 'googletagmanager.com' not in page and 'hm.baidu.com' not in page
  assert 'blob:' in csp
 with urllib.request.urlopen(base+'/res/jszip.min.js') as response: assert response.status==200
 assert request('/api/health')[0]==200
 assert request('/api/health',origin='null')[0]==403
 bridge.ALLOW_FILE=True
 assert request('/api/health',origin='null')[0]==200
 bridge.ALLOW_FILE=False
 bridge.EXTRA_ORIGIN=bridge.url_origin('https://compress.example/path')
 assert request('/api/health',origin='https://compress.example')[0]==200
 assert request('/api/health',origin='https://compress.example',token=False)[0]==200
 assert request('/api/health',origin='https://other.example')[0]==403
 bridge.EXTRA_ORIGIN=None
 assert request('/api/health',token=False,origin='null')[0]==403
 assert request('/api/health',token=False)[0]==200
 assert request('/api/health',origin='https://evil.example')[0]==403
 assert request('/api/files',b'not-jpeg',kind='image/jpeg')[0]==400
 code,result=request('/api/files',b'\xff\xd8\xffTEST\xff\xd9',kind='image/jpeg')
 assert code==201
 assert bridge.FILES[result['fileId']].read_bytes()==b'\xff\xd8\xffTEST\xff\xd9'
 import io,zipfile
 buf=io.BytesIO()
 with zipfile.ZipFile(buf,'w') as z:z.writestr('folder/sub/image.png',b'original-or-processed')
 code,archive=request('/api/archives',buf.getvalue(),kind='application/zip')
 assert code==201
 assert archive['name']=='folder.zip'
 with zipfile.ZipFile(bridge.FILES[archive['fileId']]) as z:assert z.read('folder/sub/image.png')==b'original-or-processed'
 bad=io.BytesIO()
 with zipfile.ZipFile(bad,'w') as z:z.writestr('../escape',b'no')
 assert request('/api/archives',bad.getvalue(),kind='application/zip')[0]==400
 assert request('/api/clipboard/file',json.dumps({'fileId':'unknown'}).encode())[0]==404
 server.shutdown();server.server_close();thread.join()
 print('PASS authentication, Origin, invalid JPEG, exact-byte storage, unknown clipboard ID (no clipboard write)')
