# Stage 7A: Whole-Package Modules and Imports

Status: **Historical Stage 6I handoff; refined by
[`stage-7-plan.md`](stage-7-plan.md)**

The authoritative Stage 7 design replaces this plan's flat global `ModulePath` bag,
absence of a root, and special `alias::function` grammar with a Zig-like rooted
module graph, per-module dependency names, and uniform dotted postfix resolution.
The deterministic ownership, visibility, cycle, diagnostic, and whole-package
lessons below remain design input where they do not conflict with that refinement.

This plan resolves the module/import contract during Stage 6I without prematurely
creating a package manager, filesystem search algorithm, serialized interface
format, or stable datapack ABI. Stage 7A will extend the completed scalar frontend
from one source unit to one complete, compiler-owned package before typed Minecraft
APIs broaden name and method resolution:

```text
owned ModuleInput values
    -> normalized PackageInput
    -> parsed modules and imports
    -> package-wide signature index
    -> resolved typed HIR package
    -> one verified Core program
    -> the existing whole-program backend
```

The compiler library never opens a path named by an import. A CLI or future build
tool discovers files first and supplies the complete set. Compilation remains whole
package and from source; separately compiled interfaces and dependencies remain
Stage 12.

Every supplied module is parsed and checked, including a module that nothing imports.
Imports govern name visibility and dependency reporting; they are not lazy file
discovery or a reachability root. The package has no distinguished source root, and
all `export fn` declarations form its supported datapack-entry set.

## Selected contract

### Logical identity is not a diagnostic filename

The library receives each unit as two independent values:

```text
ModuleInput {
  module: ModulePath
  source: SourceInput { name, text }
}

PackageInput {
  modules: Vec<ModuleInput>
}
```

`ModulePath` is the semantic identity. `SourceInput::name()` is display-only metadata
for diagnostics. Moving a source, changing an absolute checkout prefix, or replacing
`cells.mdl` with an editor buffer named `<cells>` cannot change resolved symbols,
Core IDs, generated resources, or emitted bytes when the supplied `ModulePath` is
unchanged.

The first `ModulePath` grammar is a nonempty `::`-separated sequence of the existing
ASCII identifier spelling, excluding language keywords. The leading component
`minecraft` is reserved for Stage 7B's explicit compiler-intrinsic namespace:

```text
prison
prison::cells
prison::cells::commands
```

These are structured flat keys within the one package, not implicit nesting or a
package prefix. `a` and `a::b` may both exist, but neither gains access to the other,
and importing one does not import descendants. Stage 12 may qualify this key with a
stable package identity without changing its component representation.

`minecraft` is also reserved as an import alias, whether inferred from the final
component or written after `as`. Thus `import tools::minecraft as mc;` is valid but
`import tools::minecraft;` and `import tools as minecraft;` are rejected. This keeps
every `minecraft::operation` spelling unambiguously compiler-owned without banning
the component from unrelated positions inside a logical module path.

The owned input is authoritative. There is no second `module ...;` declaration in a
source file and therefore no possible manifest/header disagreement. The human-facing
driver may derive a module path from a project-relative filename or obtain it from a
manifest, but that is driver policy. A later stable package convention may add a
checked source header if it has a demonstrated consumer.

The existing `compile_source(SourceInput, ...)` remains a convenience adapter. It
wraps the unit in the synthetic, non-ABI logical module `main` and calls the package
pipeline. It is not a separate parser or checker.

This differs deliberately from OCaml, which derives a compilation-unit name from
the filename, and from GHC's ordinary filename search convention. Rust demonstrates
that logical module paths and physical source paths can differ, while Zig's build
model makes the module graph an explicit compilation input. The explicit API gives
the library Zig-like ownership without importing Zig's compile-time discovery model.

### Source syntax

The module tranche adds these tokens and forms:

```text
"import" "as" "pub" "export" "::"

module       := import* function* EOF
import       := "import" module_path ("as" IDENT)? ";"
module_path  := IDENT ("::" IDENT)*
function     := visibility? "fn" IDENT "(" parameters? ")" result? block
visibility   := "pub" | "export"
call_target  := IDENT | IDENT "::" IDENT
```

Examples:

```text
import prison::users;
import prison::numbers as numbers;

fn local_helper(value: Int32) -> Int32 { return value; }

pub fn choose(value: Int32) -> Int32 {
    return numbers::clamp(value);
}

export fn run(value: Int32) -> Int32 {
    return users::normalize(choose(value));
}
```

An import is module-only. Without `as`, it binds the final path component (`users`
above). With `as`, it binds only the explicit alias. A cross-module call is exactly
`alias::function`; there are no item imports, re-exports, glob imports, relative
`self`/`super` paths, implicit preludes, or arbitrarily long expression paths in this
tranche. A same-module call remains the existing bare identifier form.

Import aliases occupy their own module-alias namespace. Because `helper()` and
`helper::run()` are syntactically distinct, a function and module alias may share a
spelling; two aliases may not. There is no unused-import diagnostic in the first
implementation.

This closed syntax has three useful properties:

- imports state the complete module dependencies instead of making any canonical
  path globally reachable;
- an alias cannot change a function's canonical identity; and
- `.` remains available for Stage 7 field and method access such as
  `player.say(text)`.

The parser stores path components and their exact spans. It does not concatenate a
path into an unchecked string. `ModulePath` owns canonical identity outside syntax;
AST import paths retain source spelling for diagnostics.

### Visibility and datapack export are different questions

Functions have three source visibility states:

| Source form | Same module | Imported module | Supported datapack entry |
| --- | --- | --- | --- |
| `fn` | yes | no | no |
| `pub fn` | yes | yes | no |
| `export fn` | yes | yes | yes |

`export` implies package visibility; `pub export` is not a form. Plain functions are
private by default. Stage 7A source fixtures that need server invocation must be
updated to say `export fn`; silently exporting every function would keep the current
temporary lowering behavior as a permanent language rule.

Package visibility is a source/HIR concern. Once all cross-module references have
resolved, both private and `pub` functions are internal Core definitions. Core gains
only the linkage distinction needed by whole-program optimization:

```text
CoreFunctionLinkage = Internal | DatapackExport
```

The verifier checks linkage, and optimizations treat `DatapackExport` functions as
having unknown external callers. Lowering still needs physical mcfunctions for
internal calls, but only exported functions appear in the supported public-entry
map and as target-cost roots. Generated resources for non-exported functions are an
implementation detail even though Minecraft has no access-control mechanism that
can prevent a handwritten pack from spelling their current resource IDs.

Stage 7A does not promise a stable resource spelling for `export fn`. The compilation
output and CLI mapping identify the generated entry for this build. Stable package
namespaces, exported resource spelling, compatibility, and separately compiled ABI
belong to Stage 12.

### Duplicate and import rules

The package checker applies these rules before checking bodies:

- each `ModulePath` occurs exactly once;
- function names are unique within their owning module; the same function spelling
  in different modules is valid;
- an import target must exist in the complete `PackageInput`;
- importing the current module is rejected;
- one target module may be imported only once by a source module;
- import aliases are unique within a module; and
- the reserved compiler-intrinsic alias `minecraft` is rejected;
- cross-module references may name only `pub` or `export` functions.

A duplicate module path is a package-input diagnostic anchored at the empty start
span of the later normalized source, with the first source as a supporting label.
The message includes the logical path because it is not written in either file.
Duplicate functions/imports use the ordinary duplicate-label policy. Unknown-module,
unknown-alias, unknown-function, and private-function diagnostics are distinct so a
misspelled module does not cascade into an arbitrary missing item.

An invalid or duplicate import is poisoned after its primary diagnostic. Uses of
that alias still traverse arguments for independent errors but do not emit repeated
unknown-name findings. The same principle already used by the scalar checker applies
to duplicate function spellings.

### Import cycles are valid in this language tranche

All package function signatures are collected before any body is checked. There are
no module initializers, top-level executable expressions, global mutable values,
type definitions, instances, or compile-time evaluation in Stage 7A. Consequently an
import cycle does not imply an initialization order and mutual recursion across
modules has a direct, deterministic meaning:

```text
a imports b; a::f calls b::g
b imports a; b::g calls a::f
```

Such cycles are accepted by the frontend. The existing Minecraft backend may still
reject a recursive reachable Core call graph through its ordinary typed
`LoweringFailure`; module resolution must not misreport that target restriction as a
cycle error.

Zig explicitly permits dependency loops between compilation modules. GHC requires a
special interface mechanism to break source cycles, and OCaml's runtime unit
initialization makes object order observable. Those extra restrictions solve
features this tranche does not have. If top-level initialization or separately
compiled interfaces arrive later, their roadmap item must revisit cycle legality
instead of changing it incidentally.

## Deterministic package normalization

The caller's vector order and filesystem traversal order are not semantic. Before
allocating `FileId`, the package frontend:

1. validates each `ModulePath` structurally;
2. sorts modules lexicographically by path components and their UTF-8 bytes;
3. uses diagnostic name, then source bytes, only to order an already-invalid
   duplicate-path group; and
4. installs sources in that normalized order.

Valid packages therefore receive the same `FileId`, `OriginId`, `SourceModuleId`,
`SourceFunctionId`, Core `FunctionId`, dumps, diagnostics, traces, and emitted bytes
under every permutation of the same module set. Within one module, imports,
functions, parameters, locals, and diagnostics remain in source order. Observable
ordering is:

```text
frontend phase -> canonical module path -> source traversal -> attached labels/notes
```

Hash maps accelerate membership and lookup only. They never allocate IDs, choose a
duplicate winner, order diagnostics, drive dumps, or select output.

`FrontendLimits::max_tokens` and `max_diagnostics` become package-wide budgets;
syntax depth remains per source. Add a separate `max_modules` (initial default
4,096) so a package of empty units cannot evade the token budget through EOF-only
files. Reaching any aggregate limit produces the existing one-final-truncation
contract and prevents HIR publication.

## Phase representation and identities

The package frontend adds only the structures required by a consumer:

```text
ModulePath                       // validated owned semantic key
SourceModuleId                   // dense canonical-module-order ID
PackageInput / ModuleInput       // complete owned input

AstPackage {
  modules: [AstModule]
}
AstModule {
  id: SourceModuleId
  imports: [AstImport]
  functions: [AstFunction]
  source: FileId
}

PackageIndex {
  modules_by_path
  module_scopes                  // import aliases and function names
  functions_in_global_id_order
}

HirPackage {
  modules
  functions                     // globally dense canonical order
}
```

`SourceFunctionId` stays globally dense, allocated by canonical module order then
function declaration order. Every HIR function records its owning `SourceModuleId`,
visibility, and canonical function path. Every resolved cross-module call contains
only `SourceFunctionId`; aliases and source spellings do not survive as semantic
identity. Local IDs remain function-owned.

Package construction has three explicit semantic passes:

1. build the complete module index and diagnose duplicate module identities;
2. collect every function signature and resolve each module's import scope; and
3. check bodies in canonical module/function order against the frozen index.

This is a proportional symbol-table design, not a generic query engine or MLIR-like
nested symbol framework. MLIR's distinction between symbol identity, references,
and visibility is useful; its extensible operation infrastructure is unnecessary for
one closed package and one item kind.

Successful checked output exposes read-only lookups for:

- `SourceModuleId -> ModulePath` and diagnostic `FileId`;
- `(ModulePath, function spelling) -> SourceFunctionId`;
- `SourceFunctionId -> owning module, visibility, signature`; and
- exported source function -> Core function -> generated datapack ABI entry.

Dense IDs remain compilation-local. Dumps print canonical module/function paths, not
only numeric IDs. `SourceToCoreMap` retains one entry per source function, while a
separate export iterator filters the supported datapack ABI; the map is not copied
into each module.

Once parsing finishes, token buffers are dropped; once checking finishes, the AST
and package index are dropped. The owned compilation output retains sources, checked
HIR, correlation maps, and the already-established backend products, not every
temporary representation.

## Compiler API versus CLI/filesystem policy

`mdl-compiler` accepts only complete owned inputs. In particular it does not:

- resolve an import by opening a file;
- use the current working directory;
- canonicalize host paths or follow symlinks;
- read a manifest, environment variable, package cache, or network dependency;
- infer module identity from `SourceInput::name()`; or
- write compiler artifacts.

A multi-file CLI tranche may accept an explicit `logical::module=path.mdl` mapping or
a project manifest, read all files, reject duplicate host inputs, and then call
`compile_package`. A convenience convention may derive module paths from paths under
one declared source root, but the fully derived mapping must be materialized before
calling the library. Import closure does not authorize the CLI to discover arbitrary
files beyond that declared input policy.

Stage 12 owns dependency resolution, package IDs, source roots, manifests, caches,
separate compilation, interface files, resource namespace stability, and cross-pack
ABI compatibility. Stage 7A intentionally provides a whole-package unit that those
systems can later feed.

## Failure ownership

`CompilationFailure` package variants retain the normalized `SourceContext` and all
ordinary diagnostics just as the scalar façade does. Add structured input failures
only for facts that cannot be source diagnostics, such as an invalid programmatically
constructed `ModulePath` or module/source identity-space exhaustion. Duplicate
logical modules, imports, visibility violations, and unresolved paths are normal
source/package diagnostics.

A syntax error in one module prevents semantic checking for the entire package. A
semantic error in one body does not prevent checking independent modules/bodies, but
no partial HIR package or Core program is published. Backend failure retains the
complete checked HIR and source/Core correlation under the existing ownership rules.

## Implementation tranches

### 7A.1 — Inputs, paths, and normalization

- implement validated `ModulePath`, `ModuleInput`, and `PackageInput`;
- add package-wide limits and deterministic normalization before `SourceContext`;
- turn `compile_source` into the `main`-module adapter; and
- prove permutation-invariant source/provenance allocation and duplicate reporting.

### 7A.2 — Syntax and recovery

- add `import`, `as`, `pub`, `export`, and `::` tokens;
- parse import lists before functions and the two-part qualified call target;
- preserve exact component/alias/modifier spans and deterministic AST dumps; and
- recover at `;`, `fn`/visibility, and EOF without swallowing the next item.

### 7A.3 — Package index and name resolution

- allocate modules and functions in canonical order;
- collect all signatures before bodies;
- resolve and poison imports deterministically;
- enforce package visibility and produce multi-source supporting labels; and
- accept import cycles while preserving the existing backend recursion failure.

### 7A.4 — HIR, Core linkage, and export maps

- extend HIR ownership/path/visibility and its verifier/dump;
- add verified Core `Internal|DatapackExport` linkage;
- preserve exported functions as unknown-root uses through Core optimization;
- distinguish internal physical entries from supported exported ABI entries in
  lowering, target-cost analysis, and the façade; and
- retain exact source/module/Core/lowered correlations.

### 7A.5 — Vertical and scale proof

- run the original single-source fixtures through the adapter;
- add multi-module forward calls, aliases, private/public/exported access, disjoint
  duplicate names, import cycles, and target recursion rejection;
- cover duplicate modules/imports/aliases, missing modules/items, and private access;
- cover the reserved `minecraft` module-prefix and import-alias cases;
- permute package input and assert exact AST/HIR/Core/report/trace/pack equality;
- prove aggregate token/diagnostic/module limits; and
- execute an exported cross-module call on the pinned Java 26.2 server under all
  four optimization-policy combinations.

Every tranche closes with formatting, warnings-denied Clippy, workspace tests,
warnings-denied rustdoc, the pinned MSRV check, and an independent verifier review.

## Explicit non-goals

- nested source modules or multiple module fragments with the same path;
- item imports, re-exports, glob imports, preludes, or import shadowing;
- packages, dependencies, version resolution, or network access;
- top-level executable initialization or global state;
- serialized interfaces, incremental queries, or separate compilation;
- stable exported resource names or cross-datapack ABI;
- filesystem discovery in `mdl-compiler`; and
- changing recursive-program target support merely because import cycles are valid.

## Primary references

- [Rust Reference: modules and module source filenames](https://doc.rust-lang.org/stable/reference/items/modules.html)
- [Rust Reference: paths and canonical paths](https://doc.rust-lang.org/reference/paths.html)
- [Rust Reference: names, scopes, and duplicate items](https://doc.rust-lang.org/reference/names/scopes.html)
- [Rust Reference: visibility and privacy](https://doc.rust-lang.org/reference/visibility-and-privacy.html)
- [Zig language reference: compilation model and module dependency graph](https://ziglang.org/documentation/master/#Compilation-Model)
- [Zig language reference: `@import`](https://ziglang.org/documentation/master/#import)
- [Zig build system: explicit root modules and dependencies](https://ziglang.org/learn/build-system/)
- [OCaml manual: batch compilation and compilation-unit naming](https://ocaml.org/manual/5.5/comp.html)
- [OCaml manual: modules](https://ocaml.org/docs/modules)
- [GHC User's Guide: filenames, module search, interfaces, and cycles](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/separate_compilation.html)
- [MLIR: symbols, symbol tables, and visibility](https://mlir.llvm.org/docs/SymbolsAndSymbolTables/)
