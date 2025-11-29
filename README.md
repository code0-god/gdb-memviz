# gdb-memviz
gdb/MI 기반으로 C/C++ 프로그램의 메모리 상태를 텍스트로 시각화하려는 실험용 도구입니다. 현재는 **Phase 2 입구** 정도로, 로컬 변수 + 심볼 단위 메모리 덤프와 타입 기반 레이아웃(`view`)을 지원하며 기본 디버깅 조작(`break/next/step/continue`)도 포함합니다. 최근 변경 사항:

- `--symbol-index-mode` 추가: `debug-only`(기본) / `debug-and-nondebug` / `none`
- 단일 소스(.c/.cc/.cpp/.cxx) 모드에서는 심볼 인덱스 파싱 시 대상 basename만 파싱하도록 최적화 (glibc 디버그 심볼 대량 파싱을 회피)
- `--log-file`만 지정해도 로그가 항상 기록됨 (`--verbose`는 stdout 미러용)
- TUI 소스 패널 전면 정리:
  - statusline 색상 독립 설정(`file_status_bg`/`file_status_fg`)
  - PC 마커·스페이서 분리 후 가터부터 하이라이트(배경만 덮어 전경 유지)
  - 패널 배경을 전체 영역에 채워 누락된 여백 제거
  - cmdline과 패널 사이 구분선 제거
  - 테마 파일(`src/tui/theme.rs`)로 syntax/마커/패널/상태라인 색상을 일원화 관리
- TUI 소스 코드 하이라이트 추가: 간단한 C/C++ 토큰(키워드/타입/문자열/숫자/주석) 컬러링을 `src/tui/highlight.rs`로 적용, 색상 팔레트는 `src/tui/theme.rs`의 `syntax_*` 값으로 조정 가능

## 실행 옵션 요약
- TUI + 디버그 심볼만(기본, 빠름): `cargo run -- --tui --symbol-index-mode debug-only --log-file perf.log examples/sample.c`
- TUI + 디버그/논디버그 전체(느림): `cargo run -- --tui --symbol-index-mode debug-and-nondebug --log-file perf.log examples/sample.c`
- TUI + 심볼 인덱스 스킵: `cargo run -- --tui --symbol-index-mode none --log-file perf.log examples/sample.c`
- 단일 소스 자동 빌드 후 실행(기본 debug-only): `cargo run -- --tui examples/sample.c`
- 바이너리 직접 실행: `cargo run -- --tui ./path/to/binary`
- CLI 모드(디버그 심볼만): `cargo run -- --symbol-index-mode debug-only --log-file perf.log examples/sample.c`
- gdb 경로/추가 로그 예시: `cargo run -- --gdb /usr/bin/gdb --verbose --log-file perf.log --tui examples/sample.c`

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

`gdb-memviz`는 gdb/MI 기반 CLI 도구를 기본으로 하지만, 터미널 기반 TUI도 실험적으로 개발 중입니다. 현재 상태는 **TUI T0.2** 수준으로, 다음과 같은 레이아웃을 제공합니다.

### 레이아웃 구조
- **헤더**: 상태 정보 한 줄 (모드, 포커스, 파일명:라인, 아키텍처, 심볼 모드)
- **본문**:
  - 좌측 (기본 60%, 조정 가능): `Source` 패널 - 소스 코드 뷰 (실제 파일 표시, PC 위치 표시, 간단한 C/C++ 하이라이트, statusline 색상 독립 설정)
  - 우측 (기본 40%, 조정 가능): `VM Layout` 패널 - 가상 메모리 레이아웃 (placeholder)
  - `Ctrl+←/→`로 좌우 비율 조정 가능 (30%~80% 범위)
- **커맨드라인**: 하단에 `:` 프롬프트 (향후 명령 입력용, 현재는 표시만)
- **Symbols 팝업**: `Ctrl+s`로 토글되는 플로팅 창 (Source 패널 우측 상단에 오버레이, 크기 조정 가능)

### 키바인딩
- **포커스 이동**:
  - `Ctrl+h`: Source 패널에 포커스
  - `Ctrl+l`: VM Layout 패널에 포커스
  - `Ctrl+s`: Symbols 팝업 열기/닫기 (열면 자동으로 Symbols에 포커스)
  - `Esc`: Symbols 팝업 닫기 (이전 포커스로 복귀)
- **레이아웃 조정**:
  - `Ctrl+←`: Source/VM 경계선 왼쪽 이동 (Source 축소, VM 확대)
    - Symbols 팝업에 포커스가 있을 때는 팝업을 왼쪽으로 확대 (우측 라인 고정)
  - `Ctrl+→`: Source/VM 경계선 오른쪽 이동 (Source 확대, VM 축소)
    - Symbols 팝업에 포커스가 있을 때는 팝업을 오른쪽으로 축소 (우측 라인 고정)
  - Source/VM 비율 범위: 30%~80%
  - Symbols 팝업 폭 범위: 20~120 칼럼 (절대값, Source/VM 크기와 독립적)
- **스크롤**:
  - `Up/Down`: 포커스된 패널 한 줄씩 스크롤 (Symbols에서는 항목 선택 이동)
  - `PageUp/PageDown`: 포커스된 패널 여러 줄 스크롤
- **Symbols 패널 내** (Symbols 팝업이 열려 있을 때):
  - `l`: locals 섹션으로 전환
  - `g`: globals 섹션으로 전환
- **디버깅**:
  - `F5`: Step over (next)
- **종료**:
  - `q` 또는 `Ctrl+c`: TUI 종료

### Run TUI (experimental)

```bash
# 단일 소스 전달 시 자동 컴파일(.c/.cc/.cpp/.cxx → <name>-memviz.out)
# 기본 심볼 모드: debug-only (glibc nondebug 제외, 빠름)
cargo run -- --tui --log-file perf.log examples/sample.c

# 전체 심볼(디버그+nondebug) 포함: 느리지만 완전
cargo run -- --tui --symbol-index-mode debug-and-nondebug --log-file perf.log examples/sample.c

# 심볼 인덱스 건너뛰기
cargo run -- --tui --symbol-index-mode none --log-file perf.log examples/sample.c

# 직접 빌드한 바이너리로 실행
gcc -g examples/sample.c -o examples/sample
cargo run -- --tui ./examples/sample
```

> **현재 상태**: Source 패널은 실제 소스 파일을 표시하고 PC 위치(▶)를 표시합니다. Symbols 팝업은 실제 locals/globals 데이터를 표시하며, 항목 선택 및 섹션 전환이 가능합니다. F5로 step over를 실행하면 실시간으로 UI가 업데이트됩니다. VM Layout은 아직 placeholder 단계입니다. 이후 단계에서 VM 캔버스, Detail 뷰, 명령 입력 등을 추가할 예정입니다.

## Build & Run
```bash
# 예제 C 프로그램 빌드
gcc -g examples/sample.c -o examples/sample

# Rust 바이너리 빌드
cargo build

# gdb-memviz 실행 (기본 gdb 사용, 로그는 파일로 기록)
cargo run -- --log-file memviz.log ./examples/sample

# 단일 소스 바로 실행 (내부에서 cc -g -O0로 빌드 후 실행, debug-only 심볼 인덱스)
cargo run -- --tui examples/sample.c

# 심볼 인덱스 모드 조정
cargo run -- --tui --symbol-index-mode debug-and-nondebug --log-file perf.log examples/sample.c
cargo run -- --tui --symbol-index-mode none --log-file perf.log examples/sample.c

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
