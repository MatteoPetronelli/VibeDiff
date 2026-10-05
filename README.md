# VibeDiff (`vd`)

> **Noise-free, token-efficient architectural diff engine powered by Tree-Sitter CST normalization and local reasoning LLMs.**

VibeDiff intercepts Git code modifications, eliminates non-semantic noise (whitespace, indentation shifts, and comments) via Tree-Sitter concrete syntax tree (CST) analysis, and streams real-time architectural evaluations through a local Ollama reasoning model adhering strictly to a 4-Pillar systems engineering schema.

---

## Key Capabilities

- **CST-Driven Noise Rejection**: Bypasses unified diff line-matching; normalizes syntax trees across baseline (`HEAD`) and mutated buffers to discard comment edits, indentation changes, and blank lines before contacting the LLM.
- **Contract Break vs. Internal Logic Classification**: Categorizes syntax modifications into `ContractBroken` (mutated public signatures, parameter types, or return types) versus `Modified` (internal body changes), `Added`, or `Deleted`.
- **Zero VRAM Spillover on 6 GB GPUs**: Optimized specifically for consumer GPUs (e.g., NVIDIA GeForce RTX 3060 6 GB VRAM) using 4B quantized reasoning models (`~60.30 tokens/sec` sustained throughput vs. 7B memory spillover stalls).
- **Headless Streaming SSE Client**: Directly consumes Ollama's OpenAI-compatible `/v1/chat/completions` API via chunked Server-Sent Events with unbuffered terminal flushing.
- **Interactive Filesystem Watcher**: Background file watcher with a 500 ms debouncer that continuously evaluates code changes upon file save.
- **Multi-Language Grammar Support**: Embeds native Tree-Sitter grammars for Rust, C#, Python, TypeScript, JavaScript, C++, and Go.

---

## System Architecture / Tech Stack

```mermaid
flowchart TD
    subgraph Ingestion ["1. Git Ingestion Layer (git2)"]
        A[Git Working Directory / Index] -->|Discover Repo| B{CLI Selection}
        B -->|Default| C[Extract Unstaged Changes]
        B -->|--staged| D[Extract Staged Index Changes]
        C --> E[Blob & Buffer Extractor]
        D --> E
        E -->|Filter Binaries, Locks, Deleted| F[Changed Files Stream]
    end

    subgraph Normalization ["2. AST Normalizer & Diff Engine (tree-sitter)"]
        F --> G[Language Parser Registry]
        G -->|Grammar Dispatch| H[CST Traversal & Tokenizer]
        H -->|Strip Comments & Whitespace| I[Normalized Symbol Extractor]
        I --> J[Hunk Signature & Body Comparator]
        J --> K{Change Classifier}
        K -->|Signature Mutated| L["AstChangeKind::ContractBroken"]
        K -->|Body Only| M["AstChangeKind::Modified"]
        K -->|New Symbol| N["AstChangeKind::Added"]
        K -->|Removed Symbol| O["AstChangeKind::Deleted"]
        L & M & N & O --> P[Token-Optimized LLM Payload Generator]
    end

    subgraph Reasoning ["3. Headless Ollama Bridge (reqwest / SSE)"]
        P --> Q["Ollama API: /v1/chat/completions"]
        R["4-Pillar System Prompt"] --> Q
        Q -->|HTTP POST JSON Stream| S["bench-reason-4b (Ollama Daemon)"]
        S -->|Server-Sent Events (SSE) Stream| T[Delta Token Parser]
    end

    subgraph Presentation ["4. Terminal Dashboard (crossterm / colored)"]
        T -->|Real-Time Unbuffered Flush| U[Streaming Terminal Console]
        V["--watch Mode (notify)"] -->|Debounced FS Events| B
    end
```

### Component Breakdown

| Layer | Technology | Crate / Tool Version | Purpose |
| :--- | :--- | :--- | :--- |
| **Git Ingestion** | `git2` | `0.19` | Discovers repository root, extracts working tree buffers and `HEAD` blobs, filters binary files and lockfiles. |
| **AST Normalization** | `tree-sitter` | `0.22` | Generates CSTs for baseline and mutated files; normalizes whitespace; strips comments; extracts symbol signatures and bodies. |
| **Grammar Bindings** | `tree-sitter-*` | `0.21` | Native C grammars for Rust, C#, Python, TypeScript, JavaScript, C++, and Go. |
| **Async Runtime** | `tokio` | `1.38` | Multi-threaded async engine powering SSE streaming, timer debouncing, and Unix/Windows signals. |
| **HTTP / SSE Client** | `reqwest` | `0.12` | Connects to Ollama REST API; parses chunked HTTP responses; extracts streaming JSON deltas. |
| **CLI & Flags** | `clap` | `4.5` | Derives declarative command-line interface arguments, defaults, and automated help/version emission. |
| **Terminal UI** | `crossterm` / `colored` | `0.27` / `2.1` | Formats colored terminal banners, status indicators, and handles unbuffered stdout streaming. |
| **Filesystem Watcher**| `notify` | `6.1` | Watches directory recursively for file write events; debounces repeated events before triggering pipeline runs. |

### API & Network Protocol Specification

VibeDiff communicates with the local Ollama daemon through the OpenAI-compatible HTTP interface:

| Endpoint | HTTP Method | Request Payload | Response Type | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| `/api/tags` | `GET` | None | `application/json` | Health check probe (3 s timeout) to verify daemon status. |
| `/` | `GET` | None | `text/plain` | Fallback health probe if `/api/tags` is restricted. |
| `/v1/chat/completions` | `POST` | `ChatCompletionRequest` (JSON) | `text/event-stream` (SSE) | Dispatches 4-Pillar system prompt and serialized AST payload; receives streaming token deltas. |

#### Client Request Schema (`ChatCompletionRequest`)
```json
{
  "model": "bench-reason-4b",
  "messages": [
    {
      "role": "system",
      "content": "You are an expert systems engineer and software architect analyzing code diffs..."
    },
    {
      "role": "user",
      "content": "FILE: src/git.rs [rust]\n--- SYMBOL: as_str [CONTRACT_BROKEN] ---\n<<< OLD\npub fn as_str(&self) -> &'static str\n>>> NEW\npub fn as_str(&self, uppercase: bool) -> &'static str\n"
    }
  ],
  "temperature": 0.2,
  "stream": true
}
```

---

## Capabilities & Supported Language Matrix

VibeDiff targets primary functional symbols across 7 languages:

| Language | Extensions | Primary CST Node Types Targeted | Contract Break Detection Rules |
| :--- | :--- | :--- | :--- |
| **Rust** | `.rs` | `function_item`, `impl_item`, `trait_item`, `struct_item`, `enum_item` | Parameter list, return type, or visibility modifier (`pub`) mutations. |
| **C#** | `.cs` | `method_declaration`, `constructor_declaration`, `property_declaration`, `class_declaration`, `interface_declaration`, `struct_declaration`, `enum_declaration` | Method signatures, access modifiers (`public`, `protected`), parameter types. |
| **Python** | `.py` | `function_definition`, `async_function_definition`, `class_definition` | Parameter names, default argument values, return type annotations. |
| **TypeScript** | `.ts`, `.tsx` | `function_declaration`, `method_definition`, `arrow_function`, `class_declaration`, `interface_declaration`, `type_alias_declaration` | Parameter types, optional markers (`?`), return type declarations. |
| **JavaScript** | `.js`, `.jsx`, `.mjs`, `.cjs` | `function_declaration`, `method_definition`, `arrow_function`, `class_declaration` | Parameter count, exported binding declarations. |
| **C++** | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.h` | `function_definition`, `class_specifier`, `struct_specifier` | Method qualifiers (`const`, `noexcept`, `override`), parameter signatures. |
| **Go** | `.go` | `function_declaration`, `method_declaration`, `type_declaration` | Method receiver type, parameter list, return signature. |

### Automated Noise Suppression
- **Ignored Files**: Lockfiles (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`), build artifacts (`target/`, `node_modules/`, `bin/`, `obj/`, `dist/`), hidden directories (`.git/`), and untracked binaries.
- **Unmodified Signatures**: Internal refactors that preserve parameter names, types, and return contracts are classified as `MODIFIED` rather than contract breaks.
- **Pure Formatting Changes**: Files with whitespace adjustments or comment insertions produce zero hunks; VibeDiff reports `"No structural AST changes detected."` and exits without executing LLM inference.

---

## The 4-Pillar Systems Engineering Schema

Every prompt payload enforces the 4-Pillar system prompt:

1. **THE DATA JOURNEY**: Step-by-step trace of how data enters, mutates, and exits the changed subsystem.
2. **ARCHITECTURAL PATTERN & DESIGN INTENT**: Explicit identification of patterns applied (Event Bus, State Machine, ECS, Guard Clause, Pre-emptive Refactoring, etc.).
3. **LANGUAGE & FRAMEWORK CAVEATS**: Ecosystem hazards (Unity C# GC allocations/hot-paths, Unreal C++ UPROPERTY ownership, Rust borrow bounds, Python GIL contention).
4. **CRITICAL ANCHORS & UNHANDLED EDGE CASES**: Bounds checks, unhandled exceptions, dropped guard clauses, or silent failure modes.

---

## Local VRAM Envelope & Reasoning Benchmark

VibeDiff is engineered to run locally on consumer workstations constrained by a **6 GB VRAM budget** (e.g., NVIDIA GeForce RTX 3060 Laptop GPU).

### Empirical Benchmark: 4B Reasoning vs. 7B Coder

Extensive profiling comparing quantized 4B reasoning models against standard 7B coder models demonstrates why a dedicated 4B reasoning model is the optimal choice for local engineering diff review:

| Benchmark Parameter | `bench-reason-4b` (Qwen 2.5 / DeepSeek 4B Distill) | `bench-coder-7b` (Qwen 2.5 Coder 7B Q4_K_M) | Impact on Local Engineering Workflow |
| :--- | :--- | :--- | :--- |
| **Model Disk Size** | `3.3 GB` | `4.7 GB` | 30% lower disk footprint. |
| **VRAM Consumption (Weights + 4K KV Cache)** | **`4.1 GB` (Fits within 6 GB VRAM)** | **`6.2 GB` (Exceeds 6 GB VRAM budget)** | 4B model stays entirely resident in VRAM. |
| **System RAM Spillover** | **`0 MB` (Zero spillover)** | **`1.4 GB` shared RAM spillover** | 7B model forces PCIe bus memory thrashing. |
| **Time to First Token (TTFT)** | **`< 0.85 s`** | **`4.82 s`** | Immediate interactive feedback on file save. |
| **Sustained Generation Speed** | **`~60.30 tokens/sec`** | **`~3.40 tokens/sec` (Stall)** | **17.7x throughput advantage**. |
| **GPU Utilization (Compute Engine)** | `98%` (Tensor Core saturation) | `< 25%` (Stalled waiting for PCIe bus) | Maximum hardware efficiency. |

### Recommended Modelfile Specification

To build the benchmarked `bench-reason-4b` model locally:

```dockerfile
FROM qwen3.5:4b

PARAMETER temperature 0.2
PARAMETER num_ctx 4096
PARAMETER stop "<|im_end|>"

SYSTEM """You are an expert systems engineer and software architect analyzing code diffs.
Produce an architectural analysis strictly adhering to the following 4-Pillar schema:

1. THE DATA JOURNEY: Step-by-step trace of how data enters, mutates, and exits the changed subsystem.
2. ARCHITECTURAL PATTERN & DESIGN INTENT: Explicit identification of patterns applied (Event Bus, State Machine, ECS, Guard Clause, etc.).
3. LANGUAGE & FRAMEWORK CAVEATS: Ecosystem hazards (Unity C# GC/hot-paths, Unreal C++ UPROPERTY ownership, Rust borrow bounds, Python GIL).
4. CRITICAL ANCHORS & UNHANDLED EDGE CASES: Bounds errors, unhandled exceptions, dropped guard clauses, or silent failures."""
```

Register the model in Ollama:

```powershell
ollama create bench-reason-4b -f Modelfile
```

---

## Prerequisites & Environment Setup

### System Requirements
- **Rust Toolchain**: `1.75+` (`rustc`, `cargo`)
- **Git**: `2.30+` installed and available in system `PATH`
- **Ollama**: Local daemon installed and listening (`ollama serve`)
- **C/C++ Build Tools**:
  - **Windows**: MSVC C++ Build Tools (via Visual Studio Build Tools with Windows 10/11 SDK).
  - **Linux**: `build-essential` (`gcc`, `g++`, `make`).
  - **macOS**: Xcode Command Line Tools (`xcode-select --install`).

---

## Installation & Quickstart

### 1. Build and Global Installation

Compile the optimized release binary and install it globally into `~/.cargo/bin`:

```powershell
# Clone the repository
git clone https://github.com/MatteoPetronelli/VibeDiff.git
cd VibeDiff

# Install globally to cargo bin
cargo install --path .
```

Verify global availability:

```powershell
vd --version
# Output: vd 0.1.0

vd --help
```

### 2. Development Execution

To execute directly from source without installing:

```powershell
# Analyze unstaged working tree changes
cargo run --bin vd

# Analyze staged index changes with custom model
cargo run --bin vd -- --staged --model bench-reason-4b
```

---

## Configuration & Environment Variables

| Variable / Flag | Location / Scope | Description | Default | Required |
| :--- | :--- | :--- | :--- | :--- |
| `-s, --staged` | CLI Flag | Evaluate staged Git index changes rather than unstaged working tree changes. | `false` | No |
| `-m, --model` | CLI Option | Target Ollama model identifier. | `bench-reason-4b` | No |
| `-e, --endpoint` | CLI Option | HTTP endpoint URL for Ollama daemon. | `http://localhost:11434` | No |
| `-w, --watch` | CLI Flag | Launch persistent filesystem watcher mode. | `false` | No |
| `OLLAMA_HOST` | Environment Variable | Host address and port bound by the Ollama server. | `127.0.0.1:11434` | No |
| `OLLAMA_FLASH_ATTENTION` | Environment Variable | Enable Flash Attention in Ollama to minimize VRAM usage. | `false` | No |

---

## Workflows & Operational Guide

### Operational Scenarios

#### Scenario 1: Unstaged Working Tree Changes
Inspect all uncommitted modifications currently in the working directory:

```powershell
vd
```

When modifications contain only whitespace or comments:
```
No structural AST changes detected.
```

When structural AST changes exist:
```
=== VibeDiff Architectural Analysis ===
src/git.rs [rust] -> as_str: CONTRACT_BROKEN, fmt: MODIFIED
----------------------------------------
1. THE DATA JOURNEY
Data enters via an external caller invoking the Display trait implementation (fmt)...
```

#### Scenario 2: Staged Git Index Changes
Inspect changes staged for the upcoming commit (`git add`):

```powershell
vd --staged
```

#### Scenario 3: Real-Time Watcher Mode
Maintain a persistent terminal dashboard that automatically re-evaluates upon file modification:

```powershell
vd --watch
```

#### Scenario 4: Custom Remote Ollama Instance or Alternative Model
Connect to a remote GPU server running an alternative model:

```powershell
vd --endpoint http://192.168.1.100:11434 --model deepseek-r1:7b
```

---

## Development, Testing & Verification

VibeDiff includes an automated test suite verifying Git ingestion, Tree-Sitter normalization, and SSE stream parsing.

### Verification Targets

| Command | Target / Scope | Success Criteria |
| :--- | :--- | :--- |
| `cargo test` | Full test suite (15 unit and integration tests) | `15 passed; 0 failed; 0 ignored` |
| `cargo check` | Type check and dependency resolution | Exits with code `0`, 0 warnings |
| `cargo build --release --bin vd` | Release binary compilation | Outputs optimized `target/release/vd.exe` |

### Key Test Suite Coverage
- `git::tests::test_synthetic_git_workflow`: Creates isolated Git repositories in temporary directories; validates working-tree blob extraction, staged index diffing, and deletion filtering.
- `ast::tests::test_rust_signature_mutation_contract_break`: Verifies that mutating parameter lists or return signatures marks the hunk as `ContractBroken`.
- `ast::tests::test_csharp_formatting_and_comment_rejection`: Asserts that injecting XML doc comments and indentation shifts yields 0 hunks.
- `client::tests::test_sse_chunk_parser`: Verifies SSE chunk fragmentation, line reconstruction, and JSON delta token extraction.
- `tests::test_path_filter_rules`: Validates path exclusion rules for `.git`, `target/`, and lockfiles.

---

## Troubleshooting & Diagnostics

### 1. Connection Refused (`http://localhost:11434`)
- **Symptom**: `Ollama daemon unreachable at http://localhost:11434. Start the service with 'ollama serve' or launch the Ollama application.`
- **Root Cause**: The Ollama background process is not running.
- **Remedy**:
  ```powershell
  # Start Ollama service in a separate terminal
  ollama serve
  ```

### 2. Model Not Found (`bench-reason-4b`)
- **Symptom**: `Ollama API returned error 404: model 'bench-reason-4b' not found`
- **Root Cause**: The custom model tag has not been registered in Ollama's local store.
- **Remedy**:
  ```powershell
  # Pull base model and initialize candidate
  ollama pull qwen3.5:4b
  ollama create bench-reason-4b -f Modelfile
  ```

### 3. VRAM Allocation Spillover / Generation Stalls
- **Symptom**: Token generation speed drops below 5 tokens/sec; high shared GPU memory usage in Task Manager.
- **Root Cause**: Model size exceeds available physical VRAM (6 GB), forcing weights into system RAM over the PCIe bus.
- **Remedy**: Switch to a quantized 4B parameter model (`qwen3.5:4b` or `bench-reason-4b`) and ensure the context window is capped at 4096 tokens (`PARAMETER num_ctx 4096`).

### 4. Git Repository Discovery Failure
- **Symptom**: `Failed to discover Git repository: could not find repository from '.'`
- **Root Cause**: Executed outside of a Git-initialized working tree.
- **Remedy**: Navigate to a directory containing a `.git` directory or initialize one with `git init`.

### 5. Windows MSVC Linker Collision (`advapi32.lib`)
- **Symptom**: Linker error: `unresolved external symbol __imp_RegOpenKeyExA`
- **Root Cause**: `libgit2` requires the Win32 registry and security library `advapi32.lib` on Windows MSVC targets.
- **Remedy**: Built into `build.rs`:
  ```rust
  println!("cargo:rustc-link-lib=advapi32");
  ```

---

## Repository Tree

```
VibeDiff/
├── build.rs              # MSVC Win32 linker configuration (advapi32.lib)
├── Cargo.toml            # Project dependencies & binary declaration (vd)
├── LICENSE               # MIT License
├── README.md             # Production systems documentation
└── src/
    ├── lib.rs            # Library root & re-exported public modules
    ├── main.rs           # CLI entry point, argument parsing & watch mode loop
    ├── git.rs            # Git repository discovery & blob extraction engine
    ├── client.rs         # Headless Ollama SSE client & 4-Pillar prompt builder
    └── ast/
        ├── mod.rs        # Tree-sitter CST normalizer & hunk classification
        └── tests.rs      # AST unit & integration tests
```

---

## License

This project is licensed under the [MIT License](LICENSE).
