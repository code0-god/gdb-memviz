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
- `globals`: 실행 파일의 전역/정적 변수 이름/타입/값 조회
- `mem <expr> [len]`: `sizeof(<expr>)` 바이트(최대 512B) 또는 `len` 만큼 `&<expr>`에서 읽어 word 단위로 덤프 (word size는 `sizeof(void*)`), hex + ASCII, 헤더에 엔디안/arch/타입 정보 표시
- `view <symbol>`: 구조체/배열의 타입 레이아웃(필드/요소 offset·size)과 raw 덤프를 함께 표시
- `follow <symbol> [depth]`: 로컬 포인터 심볼을 따라가며 링크드 구조(struct 안의 `next` 또는 첫 포인터 필드)를 depth 단계까지 텍스트로 추적, NULL에서 종료
- VM 뷰:
  - `vm`: `/proc/<pid>/maps` 를 읽어 text/data/heap/stack/lib/anon 영역을 요약
  - `vm locate <expr>`: 표현식 주소가 어떤 VM region에 속하는지 표시
  - `vm vars`: locals/globals/포인터 대상 객체를 VM region 별로 묶어 보여줌
- `help`, `quit`
- `--gdb <path>` 또는 `GDB=<path>`로 gdb 바이너리 지정
- `--verbose` + `--log-file <path>`로 MI/TUI 디버그 로그를 파일에 기록 (TUI 화면은 깔끔하게 유지)

## Limitations (Phase 2 entry)
- `mem`은 단순 심볼/간단 표현식을 권장합니다. `mem arr[2]`, `mem node.count` 정도는 동작하지만 복잡한 표현식은 보장하지 않습니다.
- 메모리 덤프는 타입 기반 구조화 없이 raw hex + ASCII이며, 최대 512B로 잘립니다(잘리면 안내 메시지 표시).
- `vm vars`는 현재 locals/globals와 포인터 대상 힙 객체만 요약합니다. ELF 섹션/strong/weak 같은 메타데이터는 추후 확장 예정입니다.
- `break` 인자는 gdb에 그대로 전달하므로 유효한 위치 문자열을 사용해야 합니다.
- 정수/부동소수점 값 해석 컬럼은 추후 Phase에서 추가 예정입니다.
- `view`의 struct/array 파서는 단순한 케이스를 대상으로 한 최소 구현입니다. 복잡한 중첩 타입/패딩/얼라인 처리는 향후 확장 예정입니다.
- 일부 환경에서 gdb의 엔디안 정보를 가져오지 못하면 `layout: unknown-endian`으로 표시될 수 있습니다(동작에는 영향 없음).

## TUI (Experimental)

`gdb-memviz`는 gdb/MI 기반 CLI 도구를 기본으로 하지만, 터미널 기반 TUI도 실험적으로 개발 중입니다. 현재 상태는 **TUI T0.1** 수준으로, 다음과 같은 골격을 제공합니다.

- 상단 status bar
- 좌측 상단: `Source` 패널 (예제 소스 placeholder)
- 좌측 하단: `Symbols` 패널 (locals/globals placeholder)
- 우측 상단: `VM Layout` 캔버스 (가상 메모리 레이아웃 placeholder)
- 우측 하단: `Detail` 패널 (struct/메모리 뷰 placeholder)
- 패널 포커스/스크롤/리사이즈:
  - `Ctrl+h/j/k/l`: 포커스 이동 (vim 스타일)
  - `Ctrl+←/→`: 좌/우 컬럼 비율 조정
  - `Ctrl+↑/↓`: 상/하 패널 비율 조정(해당 컬럼 내)
  - `=`: 레이아웃 리셋
  - `Up/Down/PageUp/PageDown`: 포커스된 패널 스크롤/선택 이동
  - `q`: 종료
  - VS Code 통합 터미널에서 `Ctrl+화살표`가 가로채지면 키바인딩을 해제/변경하거나 외부 터미널을 사용하세요.

### Run TUI (experimental)

```bash
# 단일 소스 전달 시 자동 컴파일(.c/.cc/.cpp/.cxx → <name>-memviz.out)
cargo run -- --tui --verbose --log-file perf.log examples/sample.c

# 또는 직접 빌드한 바이너리로 실행
gcc -g examples/sample.c -o examples/sample
cargo run -- --tui ./examples/sample
```

> TUI 모드는 아직 **디버깅 데이터가 완전히 연결되기 전 단계**이며, placeholder 레이아웃과 패널 구조, 포커스/스크롤/리사이즈 등 UI 골격만 제공합니다. 이후 단계에서 CLI의 `locals`, `globals`, `vm`, `view`, `mem` 정보를 TUI 패널에 바인딩할 예정입니다.

## Build & Run
```bash
# 예제 C 프로그램 빌드
gcc -g examples/sample.c -o examples/sample

# Rust 바이너리 빌드
cargo build

# gdb-memviz 실행 (기본 gdb 사용, 로그는 파일로만 기록)
cargo run -- --log-file memviz.log ./examples/sample

# 단일 소스 바로 실행 (내부에서 cc -g -O0로 빌드 후 실행)
cargo run -- --tui examples/sample.c

# gdb 경로 지정/verbose 로그 파일 예시
cargo run -- --gdb /usr/bin/gdb --verbose --log-file perf.log ./examples/sample
```

REPL에서 사용할 수 있는 명령:
```
memviz> locals
memviz> mem node           # sizeof(node)만큼 덤프 (최대 512B)
memviz> mem arr 16         # 길이 명시
memviz> view node          # struct/array 레이아웃 + raw 덤프
memviz> break examples/sample.c:30
memviz> follow node_ptr    # 포인터 체인 탐색 (옵션 depth 생략 시 기본값)
memviz> vm                 # VM 맵 요약
memviz> vm vars            # locals/globals/포인터 대상 객체를 region별로 묶어 보기
memviz> vm locate pad      # 표현식이 속한 VM 영역 확인
memviz> next / step / continue
memviz> help
memviz> quit
```


## Roadmap / Next Phases
- 타입 정보를 활용한 구조체/배열 경계 표시, 포인터 역참조 등 richer 시각화
- 포인터 체인/링크드 구조 탐색을 통한 재귀적 메모리 뷰
- VM 전체 레이아웃(text/data/heap/stack) 시각화
- TUI 또는 Web UI 확장, 명령어 자동완성/히스토리, 스크립트 지원
- TUI Phase T1: `locals`/`globals`/`vm` 데이터를 패널에 연결
- TUI Phase T2: VM 캔버스에서 symbol 위치 하이라이트, 포인터 체인 시각화
- TUI Phase T3: 소스 라인/PC 연동, breakpoint 표시, step/continue 통합
