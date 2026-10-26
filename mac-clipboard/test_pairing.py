import tempfile
from pathlib import Path
from pairing import Pairing
with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as d:
 p=Pairing(Path(d)/'grants.json',lambda *a,**k:None)
 r=p.request('https://example.com');key=r['requestId']
 assert p.poll(key,'https://evil.com')['status']=='expired'
 assert not p.valid('https://example.com','anything')
 p.decide(key,True);answer=p.poll(key,'https://example.com');token=answer['token']
 assert p.valid('https://example.com',token)
 assert not p.valid('https://evil.com',token)
 assert p.poll(key,'https://example.com')['status']=='consumed'
 q=Pairing(Path(d)/'grants.json',lambda *a,**k:None)
 assert q.valid('https://example.com',token)
 assert token not in (Path(d)/'grants.json').read_text()
 q.revoke();assert not q.valid('https://example.com',token)
 denied=q.request('https://example.com');q.decide(denied['requestId'],False)
 assert q.poll(denied['requestId'],'https://example.com')['status']=='denied'
 print('PASS approval, origin binding, single delivery, hashed persistence, restart, revoke, denial')
