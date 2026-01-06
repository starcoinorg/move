# Loader v2 implementation plan (move vs aptos-core)

## Quick comparison (why these tasks exist)
- `move` uses a monolithic `Loader` + `ModuleStorageAdapter` with eager dependency loading and a
  cache invalidation workaround (`language/move-vm/runtime/src/loader/mod.rs`).
- `aptos-core` splits loader concerns into `ModuleStorage` v2 + loader traits and offers
  `EagerLoader` + `LazyLoader`, with metered, on-demand module loading and layout caching
  (`third_party/move/move-vm/runtime/src/storage/*`).
- `aptos-core` introduces `RuntimeEnvironment`, layout cache, script cache with metered
  dependencies, and staged module publishing to avoid cache corruption during publish.

## Kanban (top-down execution order, keep loader-v1 default path)

### Phase 1: External surface + compatibility scaffolding (no behavior change)
- Entry: Loader v1 is the only active flow; we can safely add config surfaces without changing
  runtime behavior.
- Done: Config flags exist and are plumbed end-to-end with explicit defaults; dependency metering
  semantics are documented; traversal helpers + dependency charging helpers are in place with
  tests; loader-v1 behavior remains unchanged by default.
- [ ] Add VMConfig/LoaderConfig flags to mirror loader-v2 needs: `enable_lazy_loading`,
      `enable_layout_caches`, and related toggles; default to v1 behavior. (M1)
- [ ] Plumb new config flags through MoveVM/runtime/session construction paths and keep the
      loader-v1 path as the default flow. (M1)
- [ ] Define dependency metering semantics (existing vs new, ordering guarantees, special address
      handling) and document in the gas layer. (M1)
- [ ] Extend `GasMeter` or add `DependencyGasMeter` with explicit dependency charging and update
      all in-tree meters + tests to compile and pass. (M1)
- [ ] Extend `module_traversal::TraversalContext` with visit/check helpers used for lazy metering
      (e.g., `visit_if_not_special_address`, `visit_if_not_special_module_id`). (M1)
- [ ] Add unit tests for new traversal helpers (special address behavior, double-visit, etc.). (M1)
- [ ] Factor dependency charging helpers into a dedicated module (align with
      `dependencies_gas_charging.rs`) to isolate traversal semantics. (M1)

### Phase 2: Metered access + caches while keeping eager loader flow
- Entry: Phase 1 done; dependency metering interfaces are available and stable.
- Done: Dependency metering is threaded through loader entrypoints; script/layout caches are
  implemented with cache-hit metering; metadata access routes through the loader; tests cover
  cache-hit behavior; eager loader flow remains the default.
- [ ] Thread dependency metering through current loader entrypoints (module load, type load,
      script load) without changing eager semantics. (M2)
- [ ] Add script cache with deserialized/verified states; persist by script hash in transaction
      cache and VM cache. (M2)
- [ ] Ensure script verification meters immediate dependencies even on cache hits (align with
      aptos-core lazy behavior); add tests for cache-hit metering. (M2)
- [ ] Add `ModuleMetadataLoader` interface + eager implementation; route metadata access through
      it so metering is enforced instead of bypassed by cache hits. (M2)
- [ ] Update `data_cache.rs` to request metadata via the loader and add regression tests for
      metadata-dependent resource reads. (M2)
- [ ] Introduce layout cache entries that track defining modules for re-metering on cache hits. (M2)
- [ ] Implement a layout converter that resolves struct defs via the loader and threads
      `TraversalContext` + gas metering through layout construction. (M2)
- [ ] Add tests for layout size/depth errors and cache hit semantics under lazy-style metering. (M2)

### Phase 3: Loader v2 architecture behind feature flags
- Entry: Phase 2 done; metering + caches validated under eager flow.
- Done: RuntimeEnvironment owns shared caches; ModuleStorage v2 exists; loader-v2 traits and
  Eager/Lazy loaders are wired behind feature flags; loader-v1 remains the default path.
- [ ] Introduce `RuntimeEnvironment` + `WithRuntimeEnvironment` to centralize VM config and shared
      caches (type pool, module cache, script cache, layout cache) across sessions. (M3)
- [ ] Rework loader cache ownership to live in the runtime environment and remove the current
      invalidation workaround in `loader/mod.rs`. (M3)
- [ ] Implement `ModuleStorage` v2 abstraction (unmetered access to bytes/size/deserialized/
      verified modules) and adapt `TransactionDataCache` + `ModuleStorageAdapter` to implement it. (M3)
- [ ] Add module code/cache builders to support verified/deserialized states and versioning. (M3)
- [ ] Implement loader v2 traits (`StructDefinitionLoader`, `FunctionDefinitionLoader`,
      `ModuleMetadataLoader`, `NativeModuleLoader`, `ScriptLoader`, `InstantiatedFunctionLoader`)
      and wire `EagerLoader` + `LazyLoader` with a dispatch macro. (M3)
- [ ] Replace current loader entrypoints with loader-v2 flows in interpreter/runtime/session and
      ensure native context uses metered module access. (M3)

### Phase 4: Switch-on + parity + hardening
- Entry: Phase 3 done; loader-v2 path is feature-gated and usable in tests.
- Done: Publishing uses staging storage; lazy semantics match spec (including friend rules);
  parity features (if needed) are implemented; tests cover lazy loading semantics; can enable
  loader-v2 in CI or a dedicated test suite without regressions.
- [ ] Rework module publishing to use a staging storage (no caching) and lazy-loading semantics:
      immediate dependency charging + friend-in-bundle restriction. (M3)
- [ ] (If feature parity needed) add `LazyLoadedFunction` and function value/closure handling for
      lazy resolution + serialization metering. (M3)
- [ ] Update and extend tests to cover lazy loading semantics (dependency checks, friend rules,
      type layout error behavior, script cache behavior). (M3)

## Milestones

### M1 - Foundations
- VMConfig/LoaderConfig flags + plumb through construction paths (default to loader-v1 flow).
- TraversalContext helpers + tests.
- Extract dependency charging helpers.
- Define dependency metering semantics and extend GasMeter/DependencyGasMeter.

### M2 - Metered Access + Caches
- Thread dependency metering through module/type/script load entrypoints.
- Script cache (deserialized/verified) + metered dependency charging on cache hits.
- ModuleMetadataLoader path + data_cache metadata reads + tests.
- Layout cache entries + layout converter + layout error/cache-hit tests.

### M3 - Loader v2 Architecture
- RuntimeEnvironment + shared cache ownership; remove loader invalidation workaround.
- ModuleStorage v2 + module code/cache builders.
- Loader v2 traits + Eager/Lazy loaders + dispatch.
- Replace loader entrypoints across runtime/interpreter/session and native context metering.
- Staged publishing with lazy semantics.
- LazyLoadedFunction (if needed) + full semantic test suite.
