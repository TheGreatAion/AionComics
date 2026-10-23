"""Serve the prototype on http://localhost:5179, with covers read (never written) from YACReader's cache."""
import http.server
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
COVERS = r"H:\Comics\.yacreaderlibrary\covers"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 5179


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=HERE, **kwargs)

    def translate_path(self, path):
        path = path.split("?", 1)[0]
        if path.startswith("/covers/"):
            name = os.path.basename(path)
            return os.path.join(COVERS, name)
        return super().translate_path(path)

    def end_headers(self):
        self.send_header("Cache-Control", "max-age=3600")
        super().end_headers()

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
