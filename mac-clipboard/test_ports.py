import errno,importlib.util,subprocess,tempfile,json,sys
from pathlib import Path
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('bridge',Path(__file__).with_name('server.py'));bridge=importlib.util.module_from_spec(spec);spec.loader.exec_module(bridge)
for occupied in range(6):
 calls=[]
 def bind(address,handler):
  calls.append(address[1])
  if len(calls)<=occupied:raise OSError(errno.EADDRINUSE,'occupied')
  return address
 with patch.object(bridge,'ThreadingHTTPServer',bind):
  try:
   result=bridge.bind_server();assert occupied<5 and result[1]==48731+occupied
  except OSError:assert occupied==5
 assert calls==list(range(48731,48731+min(occupied+1,5)))
# Actual subprocess: stdin EOF must stop owning GUI service and release its socket.
with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as cache:
 p=subprocess.Popen([sys.executable,'-B',str(Path(__file__).parent/'JPEG Clipboard.app/Contents/Resources/mac-clipboard/server.py'),'--port','0','--no-open','--json-events','--cache-dir',cache],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 try:
  ready=json.loads(p.stdout.readline());assert ready['event']=='ready'
  p.stdin.close();p.wait(timeout=5);assert p.returncode==0
 finally:
  if p.poll() is None:p.kill();p.wait()
print('PASS five-port fallback, all-busy error, GUI pipe EOF cleanup')
