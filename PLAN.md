# 将 Aptos `lazy loader` 完整移植到 `move` `compiler-v2` 分支

## Summary
- 以 `aptos-core/third_party/move` 的 loader v2 方案为目标，把 lazy loading 的基础设施移植到 `move`，范围限定在 `language/move-vm*` 及其测试/工具链，不包含 `aptos-move/*`、链上 feature flag、Aptos VM 适配层。
- `move` 当前仍是旧式 stateful `Loader + ModuleStorageAdapter + ModuleCache` 架构；Aptos 已拆成 `RuntimeEnvironment + ModuleStorage + Eager/Lazy Loader traits + storage implementations`。移植时直接对齐 Aptos 的分层，不在旧 `Loader` 上打补丁。
- 对外兼容策略：保留现有 `move_vm::MoveVM` 和 `Session` API 作为兼容壳，内部改接 loader v2；`VMConfig.enable_lazy_loading` 新增但默认 `false`，现有默认行为保持 eager。

## Key Changes
- 在 `language/move-vm/types` 补齐 lazy loader 前置类型与缓存抽象：移植 `code/*`、`loaded_data/struct_name_indexing.rs`，并把运行期需要共享的 module/script cache、module-bytes storage、struct-name interning 从旧 loader 私有实现中抽离出来。
- 在 `language/move-vm/runtime` 新增 `storage/` 分层，并按 Aptos 结构引入 `environment.rs`、`module_storage.rs`、`loader/{traits,eager,lazy}.rs`、`implementations/unsync_{code,module}_storage.rs`、`layout_cache.rs`、`ty_layout_converter.rs`、`ty_depth_checker.rs`、`publishing.rs`。如果某个 Aptos 文件只为 lazy loader 提供必要能力，就最小化移植；不顺带引入无关 Aptos-only 特性。
- 运行时内部改造为“共享环境 + 可替换 module storage”：
  - `MoveVM` 内部持有 `RuntimeEnvironment` 与兼容所需缓存。
  - `Session` 创建时不再直接绑定旧 `ModuleStorageAdapter`，而是包装为 `UnsyncModuleStorage`/兼容适配层。
  - 执行路径统一经 `dispatch_loader!` 选择 `EagerLoader` 或 `LazyLoader`。
- 把旧 `Loader` 中与 lazy loading 冲突的职责迁走：
  - 模块反序列化/验证迁到 `ModuleStorage`。
  - `StructNameCache` 改为 `RuntimeEnvironment` 里的 `StructNameIndexMap`。
  - 类型布局与深度计算改为经 loader trait 解析结构体定义，避免直接依赖“模块已全量入缓存”的旧前提。
- 保留旧 public surface，但改内部实现：
  - `MoveVM::new/new_with_config/new_session/new_session_with_extensions/new_session_with_extensions_and_modules/load_module` 继续可用。
  - `Session::execute_*`、`publish_module*`、`load_script`、`load_function`、metadata 读取都改走 loader v2。
  - `language/extensions/async/move-async-vm`、`move-unit-test`、`transactional-test-runner`、`test-generation` 等内部消费者不要求改调用方式，只适配新的内部构造。
- 发布与验证语义按 Aptos lazy loader 对齐：
  - eager 模式保持现有“加载并验证依赖/友元的传递闭包”。
  - lazy 模式只做局部验证，按首次触达模块收费，发布阶段只要求 immediate dependencies；循环依赖 bundle 可发布，但执行时循环调用仍报错。
  - script cache、module metadata、native `LoadModule`、类型参数解析、layout cache 命中时的 defining-modules 重计费，都按 Aptos 语义接入。
- 清理旧实现：
  - 旧 `loader/mod.rs` 中仍可复用的 `Module`/`Function`/`Script` 运行期表示保留并迁入新分层。
  - 仅当旧 API 已完全被兼容层替代时，删除重复实现；不保留两套并行加载逻辑。

## Public APIs / Types
- `VMConfig` 追加 lazy-loader 所需字段，至少包括 `enable_lazy_loading`，以及实现所必需的 layout/depth/cache 开关；保留现有字段和默认 eager 行为，不做破坏式改名或删除。
- `move-vm-runtime` 对外新增并导出 `RuntimeEnvironment`、`WithRuntimeEnvironment`、`ModuleStorage`、`AsUnsyncCodeStorage`、`AsUnsyncModuleStorage`、`BorrowedOrOwned`、`StagingModuleStorage`、`EagerLoader`、`LazyLoader`、loader traits、`dispatch_loader!`。
- `move-vm-types` 对外新增 `code::*` 抽象和 `loaded_data::struct_name_indexing::*`；原 `runtime_types::StructNameIndex` 的内部使用改统一接到新 index map，不再让旧 loader 私有缓存成为单一真相来源。
- 兼容要求：现有 `move_vm::MoveVM` / `Session` API 签名不变；新增 API 只作为能力扩展，不要求仓内调用方迁移到 Aptos 的 stateless `MoveVM` 风格。

## Test Plan
- 端到端保底：`enable_lazy_loading = false` 时，`language/move-vm/integration-tests`、`transactional-tests`、`move-unit-test`、`transactional-test-runner` 现有主要测试行为不回归。
- 新增/移植 lazy loader 测试：
  - 移植 Aptos `module_storage_tests.rs`，覆盖 deserialized/verified cache、tree/DAG traversal、cyclic deps、script loading、cache state 差异。
  - 移植 Aptos `loader_tests.rs`、`instantiation_tests.rs`、`native_tests.rs` 中与 lazy loading、metadata、native `LoadModule`、函数实例化相关的用例。
  - 补 `transactional-tests/tests/lazy_loading/*` 与 `module_publishing/*.eager-loading.exp`/baseline 对照，验证 eager/lazy 语义差异。
- 验收场景：
  - lazy 模式首次加载模块时不预加载整个传递闭包。
  - layout/type-tag/depth 计算在 lazy 模式下可按需解析结构体且 gas/traversal 一致。
  - 发布包含循环结构依赖的 bundle 在 lazy 模式可过发布、执行时报正确错误。
  - metadata/native `LoadModule`/script cache 命中时仍按定义模块集合重放计费。
- 编译验证：至少通过 `cargo test -p move-vm-runtime -p move-vm-test-utils -p move-vm-integration-tests -p move-unit-test`，并跑一轮 transactional tests。

## Current Status
- 核心 lazy loader 移植已完成：`move-vm-types` 的 `code/*` 与 `StructNameIndexMap`、`move-vm-runtime` 的 `storage/*`、`RuntimeEnvironment`、`dispatch_loader!`、`EagerLoader`、`LazyLoader`、`StagingModuleStorage` 均已落地并接入执行/发布路径。
- 对外 `MoveVM` / `Session` API 仍保持兼容，但外层调用链已经切到新的 runtime 语义：
  - `MoveVM::new_session*`、`MoveVM::load_module`
  - `Session::execute_*`、`load_script`、`load_function`
  - 发布、layout/type-tag、native `LoadModule`
  这些入口现在直接通过 `VMRuntime + loader-v2 storage` 运行，不再从外层依赖 `load_*_v2` 兼容包装。
- 结构统一已继续推进到解释器内部：
  - 新增 `ExecutionContext` 作为执行期唯一上下文，统一承载 `Loader`、`ModuleStorageAdapter` 与共享 `RuntimeEnvironment`。
  - `Resolver`、native context、debug/tracing 现在都依赖 `ExecutionContext`，不再各自单独持有 `loader + module_store` 组合。
  - 当前保留的 `Loader`/`Resolver` 只承担运行时表示与字节码解析职责，不再构成第二套模块加载 runtime。
- eager/lazy 两条语义都已经通过仓内主测试验证：
  - `cargo test -p move-vm-runtime -p move-vm-test-utils -p move-vm-integration-tests -p move-unit-test`
  - `cargo test -p move-vm-transactional-tests`
  - 最近一次完整执行结果为绿色；`move-vm-integration-tests` 中仍有 9 个历史 `ignored` 用例，原因是依赖检查已移到 Move VM 外部，不是本次移植引入的新失败。
- 测试手册与测试合约选型已整理到根目录 `LAZY_LOADER_TESTING.md`。

## Remaining Gaps
- `PLAN` 中提到的 `module_storage_tests.rs` 风格 cache-state 断言还没有完整补齐；当前测试更偏功能与行为回归，而不是细粒度 deserialized/verified cache 状态校验。
- 性能专项仍未闭环：性能专用 synthetic contracts 还没补，因此目前可以确认“功能正确、可正常运行”，但还不能把“性能提升已完成验收”视为已落地结论。

## Assumptions
- 只移植 lazy loader 直接依赖的 `third_party/move` 代码；Aptos 链上 feature wiring、`aptos-vm` 集成、prod config 不在本次范围。
- 以 `compiler-v2` 当前代码为基线，优先保证仓内所有现有调用方继续使用旧 `MoveVM/Session` API；不把本次工作扩展成全面 API 重设计。
- `enable_lazy_loading` 默认关闭；测试和后续接入方通过 `VMConfig` 显式开启。
- 若 Aptos 某些 lazy-loader 依赖同时绑定了 closure/function-value/metadata 的底层类型，按“lazy loader 可工作所需最小集合”移植，不额外追求与 Aptos 其他新特性的完全同步。
