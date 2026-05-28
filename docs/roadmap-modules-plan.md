# v0.3 Modules, Imports, And Package Basics Plan

This note scopes the smallest credible v0.3 path for modules, imports, and package basics. It is an exploration artifact only: it does not change compiler, verifier, runtime, or package semantics.

## Current State

| Area | What exists now | Gap for v0.3 |
|------|-----------------|--------------|
| Semantic graph | `NodeKind::Module`, `NodeKind::Package`, `NodeKind::Import`, `NodeKind::Export`, and `NodeKind::VersionConstraint` are already representable. | No focused resolver/checker turns those graph nodes into deterministic local module/package import decisions. |
| Package manifest | `PackageManifest` includes `exports`, `imports`, capabilities, handlers, contracts, trust, schema, provenance, verification reports, and reproducible evidence. | `ail package init` currently creates manifests from the current graph but leaves `exports` and `imports` empty. |
| Import declarations | `ImportDeclaration` records source package, imported items, and optional semver constraint. | It is package-manifest metadata only; it is not connected to local module imports or CLI project authoring. |
| Dependency resolver | `DependencyResolver` resolves package specs against registry metadata with trust, yanks, advisories, semver, schema, license, capability, and handler checks. | CLI install currently looks up a direct package/version and updates the lockfile; there is no project dependency graph resolution from manifest imports. |
| Public CLI | `ail package init/add/install/search/verify/publish/audit/advisory/yank/explain` is wired to local registry and lockfile state. | No module-oriented command or example path exists for a two-module project. |

## Minimal Vertical Slice

Deliver v0.3 as a graph-first slice, not a new compiler/runtime behavior slice.

1. Represent a tiny project with two local modules in the semantic graph.
2. Add a focused import-resolution check over existing graph/package metadata.
3. Generate package manifest `exports` and `imports` from graph nodes instead of leaving them empty.
4. Resolve package manifest imports through the existing local registry and lockfile model.
5. Surface deterministic diagnostics for missing imports, duplicate exported names, cycles, and malformed package metadata.
6. Document and test the distinction between local modules, package manifests, and runtime capability grants.

## Proposed First Implementation Slice

The first implementation should be intentionally small and non-runtime-affecting:

| Step | Change | Verification |
|------|--------|--------------|
| 1 | Add a pure module/import analysis helper that accepts `SemanticGraph` plus package registry/lockfile inputs and returns resolved imports or diagnostics. | Unit tests for success, missing import, duplicate export, and cycle. |
| 2 | Teach `package_manifest_for_current_graph` to populate `exports` and `imports` from existing graph nodes when those nodes are present. | Focused CLI/package test proving JSON manifest shape; no compile/run behavior change. |
| 3 | Add `ail package verify` or `ail doctor` output for unresolved package imports using the existing local registry. | CLI test with local registry fixture and stable JSON fields. |
| 4 | Add one docs/example fixture showing two local modules and one package import as graph metadata. | Doc/example test or focused snapshot-style assertion. |

## Non-Goals For The First Slice

- Do not add a new source language parser for modules.
- Do not change compiler lowering, WASM imports, runtime preflight, or capability grant behavior.
- Do not make package install grant capabilities.
- Do not introduce remote registry behavior.
- Do not claim full v0.3 until a two-module CLI example compiles and runs.

## Design Constraints

- Keep `ail-package` independent of `ail-verify`, `ail-runtime`, and `ail-compiler`; its current dependency direction is intentional.
- Treat `import != grant` as a hard invariant. Import resolution can bring symbols/metadata into scope, but runtime capabilities remain explicitly granted elsewhere.
- Preserve deterministic data structures and output ordering because manifests, lockfiles, and graph snapshots are hash-sensitive.
- Prefer extending existing semantic graph node kinds and package metadata before adding new file formats.

## Open Questions

- Should local module imports be validated in `ail-verify`, `ail doctor`, or a dedicated package/module command first?
- What is the stable JSON diagnostic shape for module resolution errors?
- Does v0.3 need a textual project file, or can the first release remain graph/ACL-driven with docs that say so honestly?
- Should lockfile entries record transitive package imports, or only installed top-level packages for the first slice?

## Recommended Next Step

Start with a pure resolver/checker test module around existing graph metadata. That gives v0.3 a deterministic contract for local modules and package imports before any compiler or runtime lane depends on it.
