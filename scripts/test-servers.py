import asyncio, os, sys, threading
from pyftpdlib.authorizers import DummyAuthorizer
from pyftpdlib.handlers import FTPHandler
from pyftpdlib.servers import FTPServer

def ftp():
    a = DummyAuthorizer(); a.add_user("tester", "secret", "/tmp/fast-test-servers/ftproot", perm="elradfmwMT")
    h = FTPHandler; h.authorizer = a
    FTPServer(("127.0.0.1", 2121), h).serve_forever()

def dav():
    from wsgidav.wsgidav_app import WsgiDAVApp
    from cheroot import wsgi
    cfg = {"host":"127.0.0.1","port":8081,"provider_mapping":{"/":"/tmp/fast-test-servers/davroot"},
           "simple_dc":{"user_mapping":{"*":{"tester":{"password":"secret"}}}},
           "http_authenticator":{"domain_controller":None,"accept_basic":True,"accept_digest":False,"default_to_digest":False},
           "verbose":0, "logging":{"enable":False}}
    app = WsgiDAVApp(cfg)
    wsgi.Server(("127.0.0.1",8081), app).start()

async def sftp():
    import asyncssh
    key = asyncssh.generate_private_key("ssh-ed25519")
    class S(asyncssh.SSHServer):
        def begin_auth(self, u): return True
        def password_auth_supported(self): return True
        def validate_password(self, u, p): return u == "tester" and p == "secret"
    class FS(asyncssh.SFTPServer):
        def __init__(self, chan): super().__init__(chan, chroot=b"/tmp/fast-test-servers/sftproot")
    await asyncssh.listen("127.0.0.1", 2222, server_factory=S, server_host_keys=[key], sftp_factory=FS)
    await asyncio.Event().wait()

threading.Thread(target=ftp, daemon=True).start()
threading.Thread(target=dav, daemon=True).start()
asyncio.run(sftp())
