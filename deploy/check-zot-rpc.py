"""Exercise the pinned Zot binary against a local API fixture, without paid calls."""
import http.server
import json
import os
import pathlib
import selectors
import subprocess
import sys
import tempfile
import threading
import time


class API(http.server.BaseHTTPRequestHandler):
    calls = 0

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"object": "list", "data": [
            {"id": "deepseek-flash", "object": "model", "owned_by": "fixture"}
        ]}).encode())

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        if length > 1_000_000:
            self.send_error(413)
            return
        request = json.loads(self.rfile.read(length))
        API.calls += 1
        if API.calls == 1:
            message = {"role": "assistant", "content": None, "tool_calls": [{
                "id": "fixture-write", "type": "function", "function": {
                    "name": "write", "arguments": json.dumps({"path": "result.txt", "content": "fixture"})
                }
            }]}
            finish = "tool_calls"
        else:
            message = {"role": "assistant", "content": "Fixture complete."}
            finish = "stop"
        self.send_response(200)
        streaming = request.get("stream", False)
        self.send_header("Content-Type", "text/event-stream" if streaming else "application/json")
        self.end_headers()
        if streaming:
            delta = dict(message)
            if "tool_calls" in delta:
                delta["tool_calls"][0]["index"] = 0
            chunks = [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                      {"choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
                       "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}}]
            for chunk in chunks:
                chunk.update(id="fixture", object="chat.completion.chunk", model="deepseek-flash", created=0)
                self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
            self.wfile.write(b"data: [DONE]\n\n")
        else:
            self.wfile.write(json.dumps({"id": "fixture", "object": "chat.completion", "created": 0,
                "model": "deepseek-flash", "choices": [{"index": 0, "message": message, "finish_reason": finish}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}}).encode())
        self.wfile.flush()


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), API)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="magpie-zot-fixture-") as temp:
            env = dict(os.environ, DEEPSEEK_API_KEY="fixture-not-a-real-key", ZOT_HOME=temp + "/zot")
            process = subprocess.Popen([binary, "rpc", "--provider", "deepseek", "--model", "deepseek-flash",
                "--base-url", f"http://127.0.0.1:{server.server_port}/v1", "--cwd", temp, "--max-steps", "4"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env,
                start_new_session=True)
            try:
                process.stdin.write(b'{"id":"fixture","type":"prompt","message":"Write result.txt containing fixture"}\n')
                process.stdin.flush()
                events = []
                buffered = b""
                deadline = time.monotonic() + 30
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    while time.monotonic() < deadline:
                        if not selector.select(timeout=0.2):
                            continue
                        chunk = os.read(process.stdout.fileno(), 4096)
                        if not chunk:
                            raise RuntimeError("Zot exited before done")
                        buffered += chunk
                        if len(buffered) > 262144:
                            raise RuntimeError("Oversized Zot JSON event")
                        done = False
                        while b"\n" in buffered:
                            line, buffered = buffered.split(b"\n", 1)
                            event = json.loads(line)
                            if event.get("type") == "error" or event.get("success") is False:
                                raise RuntimeError("Zot fixture reported an error")
                            events.append(event.get("type"))
                            if event.get("type") == "done":
                                done = True
                                break
                        if done:
                            break
                    else:
                        raise RuntimeError("Zot fixture timed out")
                process.stdin.close()
                if process.wait(timeout=5) != 0:
                    raise RuntimeError("Zot exited unsuccessfully")
                assert pathlib.Path(temp, "result.txt").read_text() == "fixture"
                assert {"tool_call", "tool_result", "assistant_message", "usage", "done"}.issubset(events)
                assert API.calls == 2
                print("Pinned Zot RPC, tool execution, usage events, and completion passed against local fixture")
            finally:
                if process.poll() is None:
                    import signal
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
