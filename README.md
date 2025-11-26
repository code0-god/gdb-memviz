# gdb-memviz
gdb/MI 기반으로 C/C++ 프로그램의 메모리 상태를 텍스트로 시각화하려는 실험용 도구입니다. 현재는 **Phase 2 입구** 정도로, 로컬 변수 + 심볼 단위 메모리 덤프와 타입 기반 레이아웃(`view`)을 지원하며 기본 디버깅 조작(`break/next/step/continue`)도 포함합니다.

## Features (Phase 2 entry)
- gdb를 MI 모드로 실행해 대상 프로그램을 로드하고 `main`에 브레이크포인트를 걸어 실행
- 기본 디버깅 조작: `break/b`, `next/n`, `step/s`, `continue/c`
- `locals`: 현재 프레임 로컬 변수 이름/타입/값 조회 (`-stack-list-locals 2` + 값이 없을 경우 evaluate fallback)
- `mem <expr> [len]`: `sizeof(<expr>)` 바이트(최대 512B) 또는 `len` 만큼 `&<expr>`에서 읽어 word 단위로 덤프 (word size는 `sizeof(void*)`), hex + ASCII, 헤더에 엔디안/arch/타입 정보 표시
- `view <symbol>`: 구조체/배열의 타입 레이아웃(필드/요소 offset·size)과 raw 덤프를 함께 표시
- `help`, `quit`
- `--gdb <path>` 또는 `GDB=<path>`로 gdb 바이너리 지정, `--verbose`로 MI 송수신 로그를 `stderr`에 출력 (기본 모드는 gdb/MI 잡음 숨김)

## Limitations (Phase 2 entry)
- `mem`은 단순 심볼/간단 표현식을 권장합니다. `mem arr[2]`, `mem node.count` 정도는 동작하지만 복잡한 표현식은 보장하지 않습니다.
- 메모리 덤프는 타입 기반 구조화 없이 raw hex + ASCII이며, 최대 512B로 잘립니다(잘리면 안내 메시지 표시).
- VM 전체 레이아웃(text/data/heap/stack) 시각화는 아직 포함되지 않았습니다.
- `break` 인자는 gdb에 그대로 전달하므로 유효한 위치 문자열을 사용해야 합니다.
- 정수/부동소수점 값 해석 컬럼은 추후 Phase에서 추가 예정입니다.
- `view`의 struct/array 파서는 단순한 케이스를 대상으로 한 최소 구현입니다. 복잡한 중첩 타입/패딩/얼라인 처리는 향후 확장 예정입니다.

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
Commands: locals | mem <expr> [len] | view <symbol> | break <loc> | next | step | continue | help | quit

memviz> break examples/phase1_sample.c:28
breakpoint 2 at examples/phase1_sample.c:28
memviz> continue
stopped at examples/phase1_sample.c:28 (main) | reason: breakpoint-hit
memviz> locals
0: int x = 42
1: int y = 8
2: int [5] arr = {1, 2, 3, 4, 5}
3: struct Node node = {id = 7, count = 21, name = "init", '\\0' <repeats 11 times>}
4: struct Node * node_ptr = (struct Node *) 0xffffffffe880
5: int * p = (int *) 0xffffffffe874
memviz> mem node
symbol: node (struct Node)
address: 0xffffffffe880
size: 24 bytes (requested: 24, 3 words, word size = 8)
layout: little-endian (arch=aarch64)

raw:
  +0x0000: 07 00 00 00 15 00 00 00 | ascii="........"
  +0x0008: 69 6e 69 74 00 00 00 00 | ascii="init...."
  +0x0010: 00 00 00 00 00 00 00 00 | ascii="........"
memviz> view node
symbol: node (struct Node) @ 0xffffffffe880
size: 24 bytes (word size = 8)
layout: little-endian (arch=aarch64)

fields:
  offset    size  field
  +0x0000      4  id           (int)
  +0x0004      4  count        (int)
  +0x0008     16  name         (char[16])

raw:
  +0x0000: 07 00 00 00 15 00 00 00 | ascii="........"
  +0x0008: 69 6e 69 74 00 00 00 00 | ascii="init...."
  +0x0010: 00 00 00 00 00 00 00 00 | ascii="........"
memviz> quit
```

## Roadmap / Next Phases
- 타입 정보를 활용한 구조체/배열 경계 표시, 포인터 역참조 등 richer 시각화
- 포인터 체인/링크드 구조 탐색을 통한 재귀적 메모리 뷰
- VM 전체 레이아웃(text/data/heap/stack) 시각화
- TUI 또는 Web UI 확장, 명령어 자동완성/히스토리, 스크립트 지원
