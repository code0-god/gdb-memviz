# gdb-memviz
gdb/MI 기반으로 C/C++ 프로그램의 메모리 상태를 텍스트로 시각화하려는 실험용 도구입니다. 현재는 **Phase 1** 수준으로, 로컬 변수 목록과 심볼 단위 메모리 hex dump를 제공하는 간단한 CLI를 담고 있습니다.

## Features (Phase 1)
- gdb를 MI 모드로 실행해 대상 프로그램을 로드하고 `main`에 브레이크포인트를 걸어 실행
- 기본 디버깅 조작: `break/b`, `next/n`, `step/s`, `continue/c`
- `locals` 명령으로 현재 프레임의 로컬 변수 이름/타입/값을 조회 (`-stack-list-locals 2` + fallback evaluate)
- `mem <symbol>` 명령으로 해당 심볼 주소에서 정해진 길이(기본 32바이트)를 raw hex로 덤프
- `help`, `quit` 제공
- `--gdb <path>` 또는 `GDB=<path>`로 gdb 바이너리 지정, `--verbose`로 MI 명령 송신 로그(`stderr`) 확인

## Limitations (Phase 1)
- `mem`은 기본적으로 단순 심볼 이름을 권장합니다. gdb 표현식을 그대로 넘기기 때문에 `mem arr[2]`, `mem node.count` 같은 단순 인덱스/필드 접근이 동작하긴 하지만, 복잡한 표현식에서는 보장하지 않습니다.
- 메모리 덤프는 타입 기반 구조화 없이 raw hex만 출력합니다.
- VM 전체 레이아웃(text/data/heap/stack) 시각화는 아직 포함되지 않았습니다.
- `break` 인자는 gdb에 그대로 전달하므로, 유효한 위치 문자열을 사용해야 합니다.

## Build & Run
```bash
# 예제 C 프로그램 빌드
gcc -g examples/phase1_sample.c -o examples/phase1_sample

# Rust 바이너리 빌드
cargo build

# gdb-memviz 실행 (기본 gdb 사용)
cargo run -- ./examples/phase1_sample

# gdb 경로 지정/로그 확인 예시
cargo run -- --gdb /usr/bin/gdb --verbose ./examples/phase1_sample
```

REPL에서 사용할 수 있는 명령:
```
memviz> locals
memviz> mem x
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
Reached breakpoint at main. Type 'help' for commands.
Commands: locals | mem <symbol> | break <loc> | next | step | continue | help | quit
memviz> break examples/phase1_sample.c:28
breakpoint 2 at examples/phase1_sample.c:28
memviz> continue
stopped at examples/phase1_sample.c:28 (main)
memviz> locals
0: x = 42
1: y = 8
2: arr = {1, 2, 3, 4, 5}
3: node = {id = 7, count = 21, name = "init", '\000' <repeats 11 times>}
4: node_ptr = 0xffffffffe880
5: p = 0xffffffffe874
memviz> mem x
address: 0xffffffffe850
bytes(32):
  0x00: 2a 00 00 00 08 00 00 00 80 e8 ff ff ff ff 00 00
  0x10: 74 e8 ff ff ff ff 00 00 01 00 00 00 02 00 00 00
memviz> mem arr[2]
address: 0xffffffffe870
bytes(32):
  0x00: 03 00 00 00 04 00 00 00 05 00 00 00 00 00 00 00
  0x10: 07 00 00 00 15 00 00 00 69 6e 69 74 00 00 00 00
memviz> mem node.count
address: 0xffffffffe884
bytes(32):
  0x00: 15 00 00 00 69 6e 69 74 00 00 00 00 00 00 00 00
  0x10: 00 00 00 00 00 91 44 91 1a 1d 7f 3c b0 e9 ff ff
memviz> quit
```

## Roadmap / Next Phases
- 타입 정보를 활용한 구조체/배열 경계 표시, 포인터 역참조 등 richer 시각화
- 포인터 체인/링크드 구조 탐색을 통한 재귀적 메모리 뷰
- VM 레이아웃(text/data/heap/stack) 시각화
- TUI 또는 Web UI 확장, 명령어 자동완성/히스토리, 스크립트 지원
