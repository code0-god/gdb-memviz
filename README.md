# gdb-memviz
gdb/MI 기반으로 C/C++ 프로그램의 메모리 상태를 텍스트로 시각화하려는 실험용 도구입니다. 현재는 **Phase 2 입구** 정도로, 로컬 변수 + 심볼 단위 메모리 덤프와 타입 기반 레이아웃(`view`)을 지원하며 기본 디버깅 조작(`break/next/step/continue`)도 포함합니다.

## Requirements

- Linux 환경 (gdb/MI, `/proc` 기반)
- gdb 8.x 이상 (`-gdb-show endian` 지원)
- Rust stable (cargo 빌드 가능)

## Features (Phase 2 entry)
- gdb를 MI 모드로 실행해 대상 프로그램을 로드하고 `main`에 브레이크포인트를 걸어 실행
- 기본 디버깅 조작: `break/b`, `next/n`, `step/s`, `continue/c`
- `locals`: 현재 프레임 로컬 변수 이름/타입/값 조회 (`-stack-list-locals 2` + 값이 없을 경우 evaluate fallback)
- `mem <expr> [len]`: `sizeof(<expr>)` 바이트(최대 512B) 또는 `len` 만큼 `&<expr>`에서 읽어 word 단위로 덤프 (word size는 `sizeof(void*)`), hex + ASCII, 헤더에 엔디안/arch/타입 정보 표시
- `view <symbol>`: 구조체/배열의 타입 레이아웃(필드/요소 offset·size)과 raw 덤프를 함께 표시
- `follow <symbol> [depth]`: 로컬 포인터 심볼을 따라가며 링크드 구조(struct 안의 `next` 또는 첫 포인터 필드)를 depth 단계까지 텍스트로 추적, NULL에서 종료
- `help`, `quit`
- `--gdb <path>` 또는 `GDB=<path>`로 gdb 바이너리 지정, `--verbose`로 MI 송수신 로그를 `stderr`에 출력 (기본 모드는 gdb/MI 잡음 숨김)

## Limitations (Phase 2 entry)
- `mem`은 단순 심볼/간단 표현식을 권장합니다. `mem arr[2]`, `mem node.count` 정도는 동작하지만 복잡한 표현식은 보장하지 않습니다.
- 메모리 덤프는 타입 기반 구조화 없이 raw hex + ASCII이며, 최대 512B로 잘립니다(잘리면 안내 메시지 표시).
- VM 전체 레이아웃(text/data/heap/stack) 시각화는 아직 포함되지 않았습니다.
- `break` 인자는 gdb에 그대로 전달하므로 유효한 위치 문자열을 사용해야 합니다.
- 정수/부동소수점 값 해석 컬럼은 추후 Phase에서 추가 예정입니다.
- `view`의 struct/array 파서는 단순한 케이스를 대상으로 한 최소 구현입니다. 복잡한 중첩 타입/패딩/얼라인 처리는 향후 확장 예정입니다.
- 일부 환경에서 gdb의 엔디안 정보를 가져오지 못하면 `layout: unknown-endian`으로 표시될 수 있습니다(동작에는 영향 없음).

## Build & Run
```bash
# 예제 C 프로그램 빌드
gcc -g examples/phase1_sample.c -o examples/phase1_sample

# Rust 바이너리 빌드
cargo build

# gdb-memviz 실행 (기본 gdb 사용, 로그 최소화)
cargo run -- ./examples/phase1_sample

# gdb 경로 지정/로그 확인 예시
cargo run -- --gdb /usr/bin/gdb --verbose ./examples/phase1_sample
```

REPL에서 사용할 수 있는 명령:
```
memviz> locals
memviz> mem node           # sizeof(node)만큼 덤프 (최대 512B)
memviz> mem arr 16         # 길이 명시
memviz> view node          # struct/array 레이아웃 + raw 덤프
memviz> break examples/phase1_sample.c:30
memviz> follow node_ptr    # 포인터 체인 탐색 (옵션 depth 생략 시 기본값)
memviz> next / step / continue
memviz> help
memviz> quit
```

## Example Session
```
$ cargo run -- ./examples/phase1_sample
[gdb-memviz] gdb: gdb | target: ./examples/phase1_sample [] | verbose: false

# probing gdb ...

# break main and run
Reached breakpoint at main. Type 'help' for commands.
Commands: locals | mem <expr> [len] | view <symbol> | follow <symbol> [depth] | break <loc> | next | step | continue | help | quit

memviz> break examples/phase1_sample.c:36
breakpoint 2 at examples/phase1_sample.c:36
memviz> continue
stopped at examples/phase1_sample.c:36 (main) | reason: breakpoint-hit
memviz> locals
0: int x = 42
1: int y = 8
2: int[5] arr = {1, 2, 3, 4, 5}
3: struct Node node0 = {id = 0, count = 10, name = "node0", \0 (x11), next = 0xffffffffe8b0}
4: struct Node node1 = {id = 1, count = 20, name = "node1", \0 (x11), next = 0xffffffffe8e0}
5: struct Node node2 = {id = 2, count = 30, name = "node2", \0 (x11), next = 0x0}
6: struct Node * node_ptr = 0xffffffffe880
7: int * p = 0xffffffffe874
memviz> mem node0
symbol: node0 (struct Node)
address: 0xffffffffe880
size: 32 bytes (requested: 32, 4 words, word size = 8)
layout: little-endian (arch=aarch64)

raw:
  +0x0000: 00 00 00 00 0a 00 00 00 | ascii="........"
  +0x0008: 6e 6f 64 65 30 00 00 00 | ascii="node0..."
  +0x0010: 00 00 00 00 00 00 00 00 | ascii="........"
  +0x0018: b0 e8 ff ff ff ff ff ff | ascii="........"
memviz> view node0
symbol: node0 (struct Node) @ 0xffffffffe880
size: 32 bytes (word size = 8)
layout: little-endian (arch=aarch64)

fields:
  offset    size  field
  +0x0000      4  id           (int)
  +0x0004      4  count        (int)
  +0x0008     16  name         (char[16])
  +0x0018      8  next         (struct Node*)

raw:
  +0x0000: 00 00 00 00 0a 00 00 00 | ascii="........"
  +0x0008: 6e 6f 64 65 30 00 00 00 | ascii="node0..."
  +0x0010: 00 00 00 00 00 00 00 00 | ascii="........"
  +0x0018: b0 e8 ff ff ff ff ff ff | ascii="........"
memviz> follow node_ptr 4
[0] node_ptr (struct Node*) = 0xffffffffe880
    -> struct Node { id = 0, count = 10, name = "node0", \0 (x11), next = 0xffffffffe8b0 }
[1] node_ptr->next (struct Node*) = 0xffffffffe8b0
    -> struct Node { id = 1, count = 20, name = "node1", \0 (x11), next = 0xffffffffe8e0 }
[2] node_ptr->next->next (struct Node*) = 0xffffffffe8e0
    -> struct Node { id = 2, count = 30, name = "node2", \0 (x11), next = 0x0 }
[3] node_ptr->next->next->next (struct Node*) = 0x0
    -> NULL (stopped)
memviz> quit
```

## Roadmap / Next Phases
- 타입 정보를 활용한 구조체/배열 경계 표시, 포인터 역참조 등 richer 시각화
- 포인터 체인/링크드 구조 탐색을 통한 재귀적 메모리 뷰
- VM 전체 레이아웃(text/data/heap/stack) 시각화
- TUI 또는 Web UI 확장, 명령어 자동완성/히스토리, 스크립트 지원
