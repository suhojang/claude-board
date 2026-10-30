#!/usr/bin/env python3
# 가짜 바깥 서버: 자율주행 4단계(연결·올릴 글) 시험용. 127.0.0.1 에서만 듣고, 실제 Atlassian·Notion·Figma·메신저로는 아무것도 보내지 않는다.
# 보드·엔진이 보내는 요청 꼴(경로·메서드·본문)을 받고 그럴듯한 답을 돌려주며, 받은 요청을 JSONL 로 남긴다.
# 받는 요청은 이 조각이 공식 문서를 보고 정한 꼴의 흉내다 — 진짜 서버가 같은 답을 준다는 보증이 아니다(처음 연결할 때 "연결 확인"과 첫 올리기가 그 확인).
#
# 쓰는 법: python3 -I "가짜 바깥 서버.py" --log <기록.jsonl> [--port 0] [--port-file <파일>] [--email kim@example.com]
#           [--fail429 <n>] [--expired] [--redirect <주소>] [--name <이 서버 이름>]
#   토큰: 환경 변수 FAKE_TOKEN(가짜 값, 실제 토큰을 쓰지 않는다). 인증 머리가 그 값과 맞지 않으면 401.
#   --fail429 n   처음 쓰기 요청 n 개에 429 + Retry-After: 1
#   --expired     모든 요청에 401(토큰 만료 흉내)
#   --redirect U  모든 요청에 302 Location: U(받는 쪽이 리다이렉트를 따라가는지 보려고. 따라가면 U 쪽 서버 기록에 남는다)
#
# 받는 것(시험 빌드의 사이트·바탕 주소를 http://127.0.0.1:<포트> 로):
#   Atlassian  GET /rest/api/3/myself · POST·GET /rest/api/3/issue/<키>/comment · POST /wiki/rest/api/content · GET /wiki/rest/api/content
#              PUT /wiki/rest/api/content/<id>/child/attachment(multipart, X-Atlassian-Token: no-check)
#   Notion     GET /v1/users/me · POST /v1/pages · PATCH /v1/blocks/<id>/children(Notion-Version 머리, 한 번에 100블록·글 한 칸 2,000자 확인)
#   Figma      GET /v1/me(X-Figma-Token)
#   webhook    POST /hook/<아무것>
# 기록 한 줄: {ts, server, method, path, status, auth:{scheme, ok, sha8}, bytes, checks:[…], created} — 인증 머리 값은 남기지 않는다(sha256 앞 8자만)
import argparse, base64, hashlib, json, os, re, sys, threading, time, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ap = argparse.ArgumentParser()
ap.add_argument("--log", required=True)
ap.add_argument("--port", type=int, default=0)
ap.add_argument("--port-file")
ap.add_argument("--email", default="kim@example.com")
ap.add_argument("--fail429", type=int, default=0)
ap.add_argument("--expired", action="store_true")
ap.add_argument("--redirect")
ap.add_argument("--name", default="fake")
A = ap.parse_args()
TOKEN = os.environ.get("FAKE_TOKEN", "")
if not TOKEN.startswith("fake-"):
    sys.exit("FAKE_TOKEN 은 fake- 로 시작하는 가짜 값만 받습니다")

lock = threading.RLock()   # reply 안의 log 도 같은 잠금을 잡는다(429 갈래는 잠금 안에서 reply)
state = {"n": 1000, "left429": A.fail429, "titles": set(), "comments": {}, "pages": {}, "attach": {}}


def log(entry):
    entry["ts"] = round(time.time(), 3)
    entry["server"] = A.name
    with lock, open(A.log, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False, sort_keys=True) + "\n")


def check_auth(h):
    a = h.get("Authorization") or ""
    f = h.get("X-Figma-Token") or ""
    if f:
        return {"scheme": "X-Figma-Token", "ok": f == TOKEN, "sha8": hashlib.sha256(f.encode()).hexdigest()[:8]}
    if a.startswith("Basic "):
        try:
            user, _, tok = base64.b64decode(a[6:]).decode().partition(":")
        except Exception:
            user, tok = "", ""
        return {"scheme": "Basic", "ok": tok == TOKEN and user == A.email, "user": user, "sha8": hashlib.sha256(a.encode()).hexdigest()[:8]}
    if a.startswith("Bearer "):
        return {"scheme": "Bearer", "ok": a[7:] == TOKEN, "sha8": hashlib.sha256(a.encode()).hexdigest()[:8]}
    return {"scheme": "none", "ok": False}


def notion_checks(blocks, where):
    out = []
    if len(blocks) > 100:
        out.append(f"{where}: 블록 {len(blocks)}개(100 넘음)")
    for b in blocks:
        t = b.get("type")
        rich = (b.get(t) or {}).get("rich_text", []) if isinstance(b.get(t), dict) else []
        if len(rich) > 100:
            out.append(f"{where}: rich_text {len(rich)}칸(100 넘음)")
        for r in rich:
            c = (r.get("text") or {}).get("content", "")
            if len(c) > 2000:
                out.append(f"{where}: 글 한 칸 {len(c)}자(2000 넘음)")
    return out


def adf_checks(doc):
    out = []
    if not (isinstance(doc, dict) and doc.get("type") == "doc" and doc.get("version") == 1):
        out.append("ADF 머리가 doc·version 1 이 아님")

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "text" and not n.get("text"):
                out.append("ADF 빈 글 마디")
            for c in n.get("content", []) or []:
                walk(c)
    walk(doc)
    return out


class H(BaseHTTPRequestHandler):
    server_version = "fake-outside"

    def log_message(self, *a):
        pass

    def reply(self, status, obj=None, headers=None, entry=None):
        body = b"" if obj is None else json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(status)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        if entry is not None:
            entry["status"] = status
            log(entry)

    def handle_any(self, method):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n > 0 else b""
        path = self.path.split("?")[0]
        e = {"method": method, "path": path, "bytes": len(raw), "auth": check_auth(self.headers), "checks": []}
        if A.redirect:
            return self.reply(302, None, {"Location": A.redirect + self.path}, e)
        if A.expired or not e["auth"]["ok"]:
            if not path.startswith("/hook/"):
                return self.reply(401, {"message": "Unauthorized"}, None, e)
        write = method in ("POST", "PUT", "PATCH")
        if write:
            with lock:
                if state["left429"] > 0:
                    state["left429"] -= 1
                    return self.reply(429, {"message": "Too Many Requests"}, {"Retry-After": "1"}, e)
        body = None
        ctype = self.headers.get("Content-Type") or ""
        if raw and "json" in ctype:
            try:
                body = json.loads(raw)
            except Exception:
                e["checks"].append("JSON 이 아님")
                return self.reply(400, {"message": "bad json"}, None, e)
        with lock:
            state["n"] += 1
            nid = str(state["n"])
        # Atlassian
        if method == "GET" and path == "/rest/api/3/myself":
            return self.reply(200, {"accountId": "fake-account", "displayName": "가짜 사용자", "emailAddress": A.email}, None, e)
        m = re.fullmatch(r"/rest/api/3/issue/([A-Z][A-Z0-9_]*-[0-9]+)/comment", path)
        if m and method == "POST":
            e["checks"] += adf_checks((body or {}).get("body"))
            with lock:
                state["comments"].setdefault(m.group(1), []).append(nid)
            e["created"] = {"kind": "jira-comment", "issue": m.group(1), "id": nid}
            return self.reply(201, {"id": nid, "self": f"http://127.0.0.1/rest/api/3/issue/{m.group(1)}/comment/{nid}"}, None, e)
        if m and method == "GET":
            return self.reply(200, {"comments": [{"id": c} for c in state["comments"].get(m.group(1), [])]}, None, e)
        if path == "/wiki/rest/api/content" and method == "POST":
            b = body or {}
            key = (b.get("space") or {}).get("key", "")
            title = b.get("title", "")
            stor = ((b.get("body") or {}).get("storage") or {}).get("value", "")
            if "<script" in stor.lower() or "<ac:" in stor:
                e["checks"].append("저장 형식에 script·매크로")
            with lock:
                if (key, title) in state["titles"]:
                    e["checks"].append("같은 제목")
                    return self.reply(400, {"message": "A page with this title already exists"}, None, e)
                state["titles"].add((key, title))
            e["created"] = {"kind": "confluence-page", "space": key, "id": nid, "title": title}
            return self.reply(200, {"id": nid, "type": "page", "title": title, "_links": {"webui": f"/spaces/{key}/pages/{nid}"}}, None, e)
        if path == "/wiki/rest/api/content" and method == "GET":
            return self.reply(200, {"results": []}, None, e)
        m = re.fullmatch(r"/wiki/rest/api/content/([0-9]+)/child/attachment", path)
        if m and method in ("PUT", "POST"):
            if self.headers.get("X-Atlassian-Token") != "no-check":
                e["checks"].append("X-Atlassian-Token 없음")
            fn = re.search(rb'filename="([^"]+)"', raw)
            name = fn.group(1).decode() if fn else ""
            with lock:
                again = name in state["attach"].setdefault(m.group(1), set())
                state["attach"][m.group(1)].add(name)
            e["created"] = {"kind": "confluence-attachment", "page": m.group(1), "name": name, "again": again}
            return self.reply(200, {"results": [{"id": "att" + nid, "title": name}]}, None, e)
        # Notion
        if method == "GET" and path == "/v1/users/me":
            if self.headers.get("Notion-Version") is None:
                e["checks"].append("Notion-Version 없음")
            return self.reply(200, {"object": "user", "id": str(uuid.uuid4()), "name": "Claude Board", "type": "bot",
                                    "bot": {"owner": {"type": "user", "user": {"name": "가짜 요청자"}}}}, None, e)
        if method == "POST" and path == "/v1/pages":
            if self.headers.get("Notion-Version") is None:
                e["checks"].append("Notion-Version 없음")
            e["checks"] += notion_checks((body or {}).get("children", []), "페이지")
            pid = str(uuid.uuid4())
            with lock:
                state["pages"][pid.replace("-", "")] = len((body or {}).get("children", []))
            e["created"] = {"kind": "notion-page", "id": pid, "blocks": len((body or {}).get("children", []))}
            return self.reply(200, {"object": "page", "id": pid, "url": "https://www.notion.so/" + pid.replace("-", "")}, None, e)
        m = re.fullmatch(r"/v1/blocks/([0-9a-fA-F-]{32,36})/children", path)
        if m and method == "PATCH":
            e["checks"] += notion_checks((body or {}).get("children", []), "덧붙이기")
            e["created"] = {"kind": "notion-blocks", "page": m.group(1), "blocks": len((body or {}).get("children", []))}
            return self.reply(200, {"object": "list", "results": []}, None, e)
        # Figma
        if method == "GET" and path == "/v1/me":
            return self.reply(200, {"id": "1", "email": A.email, "handle": "kim"}, None, e)
        # webhook(⑩-3 알림 흉내)
        if method == "POST" and path.startswith("/hook/"):
            e["created"] = {"kind": "webhook"}
            return self.reply(200, {"ok": True}, None, e)
        return self.reply(404, {"message": "not found"}, None, e)

    def do_GET(self): self.handle_any("GET")
    def do_POST(self): self.handle_any("POST")
    def do_PUT(self): self.handle_any("PUT")
    def do_PATCH(self): self.handle_any("PATCH")
    def do_DELETE(self): self.handle_any("DELETE")


srv = ThreadingHTTPServer(("127.0.0.1", A.port), H)
port = srv.server_address[1]
if A.port_file:
    with open(A.port_file, "w") as f:
        f.write(str(port))
print(f"가짜 바깥 서버 {A.name}: http://127.0.0.1:{port} (기록 {A.log})", flush=True)
try:
    srv.serve_forever()
except KeyboardInterrupt:
    pass
