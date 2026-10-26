"""Origin-bound pairing grants. UI communicates via stdin, never via public admin API."""
import hashlib,json,secrets,threading,time
from pathlib import Path
class Pairing:
 def __init__(self,path,emit):
  self.path=Path(path);self.emit=emit;self.lock=threading.RLock();self.pending={}
  try:self.grants=json.loads(self.path.read_text())
  except (OSError,ValueError):self.grants={}
 def save(self):
  self.path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
  tmp=self.path.with_suffix('.tmp');tmp.write_text(json.dumps(self.grants));tmp.chmod(0o600);tmp.replace(self.path)
 def request(self,origin):
  with self.lock:
   now=time.time();self.pending={k:v for k,v in self.pending.items() if now-v['time']<120}
   if len(self.pending)>=8 or any(v['origin']==origin and v['status']=='pending' for v in self.pending.values()):raise ValueError('已有配对请求，请在客户端确认或稍后重试')
   key=secrets.token_urlsafe(32);self.pending[key]={'origin':origin,'time':now,'status':'pending'}
   self.emit('pair',requestId=key,origin=origin)
   return {'requestId':key,'status':'pending'}
 def poll(self,key,origin):
  with self.lock:
   p=self.pending.get(key)
   if not p or p['origin']!=origin or time.time()-p['time']>120:return {'status':'expired'}
   result={'status':p['status']}
   if p['status']=='approved':result['token']=p.pop('token',None);p['status']='consumed'
   return result
 def decide(self,key,allow):
  with self.lock:
   p=self.pending.get(key)
   if not p or p['status']!='pending' or time.time()-p['time']>120:return
   p['status']='denied'
   if allow:
    token=secrets.token_urlsafe(32);self.grants[p['origin']]=hashlib.sha256(token.encode()).hexdigest();self.save();p.update(status='approved',token=token)
   self.emit('grants',origins=list(self.grants))
 def valid(self,origin,token):
  with self.lock:return bool(token) and secrets.compare_digest(self.grants.get(origin,''),hashlib.sha256(token.encode()).hexdigest())
 def revoke(self):
  with self.lock:
   self.grants={};self.pending={};self.save();self.emit('grants',origins=[])
