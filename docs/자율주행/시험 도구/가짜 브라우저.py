#!/usr/bin/env python3
# 가짜 브라우저 드라이버: 자율주행 시험 실행기(5단계 ⑪-1)의 브라우저 시나리오 시험용. 진짜 브라우저를 띄우지 않는다.
# 쓰는 법: 시험 실행 폴더 run.json 에 "browser_path": "<이 파일 절대 경로>"(--dev 출발만). 엔진은 브라우저 시나리오를 돌릴 때
# $CLAUDE_BOARD_BROWSER 에 이 파일을, $CLAUDE_BOARD_BROWSER_FLAGS 에 headless 플래그를 넘긴다. 시나리오는 Chrome 처럼 부른다:
#   "$CLAUDE_BOARD_BROWSER" $CLAUDE_BOARD_BROWSER_FLAGS --screenshot="$CLAUDE_BOARD_EVIDENCE_DIR/page.png" http://127.0.0.1:<포트>/
#   "$CLAUDE_BOARD_BROWSER" $CLAUDE_BOARD_BROWSER_FLAGS --dump-dom http://127.0.0.1:<포트>/
# 하는 일: 주소를 urllib 로 한 번 받아(127.0.0.1·localhost 만, 그 밖은 받지 않고 종료 코드 2) --dump-dom 이면 본문을 표준 출력에,
# --screenshot=<파일> 이면 받은 본문 길이를 적은 작은 PNG(1×1)를 그 파일에 쓴다. 받은 인자는 표준 오류에 한 줄.
# 응답이 200 이 아니면 종료 코드 1(진짜 Chrome 은 0 으로 끝나므로 시나리오가 본문을 따로 보는 것이 맞다. 여기서는 시험을 쉽게 하려고 실패로)
import struct, sys, urllib.parse, urllib.request, zlib


def png(note: bytes) -> bytes:
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    raw = b"\x00\xff\xff\xff"
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
            + chunk(b"tEXt", b"Comment\x00" + note) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def main() -> int:
    args = sys.argv[1:]
    print("가짜 브라우저 인자: " + " ".join(args), file=sys.stderr)
    urls = [a for a in args if not a.startswith("--")]
    if not urls:
        print("주소가 없습니다", file=sys.stderr)
        return 2
    url = urls[-1]
    host = urllib.parse.urlparse(url).hostname or ""
    if host not in ("127.0.0.1", "localhost"):
        print("가짜 브라우저는 127.0.0.1·localhost 만 엽니다: " + host, file=sys.stderr)
        return 2
    try:
        with urllib.request.urlopen(url, timeout=10) as r:
            body, status = r.read(), r.status
    except Exception as e:  # 받지 못함(서버 없음·막힘·404)
        print("받지 못함: %s" % e, file=sys.stderr)
        return 1
    for a in args:
        if a == "--dump-dom":
            sys.stdout.write(body.decode("utf-8", "replace"))
        if a.startswith("--screenshot="):
            with open(a.split("=", 1)[1], "wb") as f:
                f.write(png(("fake %d bytes from %s" % (len(body), url)).encode()))
    return 0 if status == 200 else 1


if __name__ == "__main__":
    sys.exit(main())
